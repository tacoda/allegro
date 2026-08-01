use std::thread;

use reqwest::blocking::Client;
use serde_json::{json, Value as Json};

// A tool call the model requested during a completion.
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

// The assistant's reply: free-text content and/or tool calls to run.
pub struct Reply {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
}

pub struct Agent {
    pub model: String,
    pub system: String,
    pub temperature: f64,
    api_key: String,
    client: Client,
}

impl Agent {
    pub fn new(
        provider: &str,
        model: String,
        system: String,
        temperature: f64,
    ) -> Result<Agent, String> {
        if provider != "openai" {
            return Err(format!(
                "provider '{}' is not supported yet (only 'openai')",
                provider
            ));
        }
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| "OPENAI_API_KEY is not set in the environment".to_string())?;
        Ok(Agent {
            model,
            system,
            temperature,
            api_key,
            client: Client::new(),
        })
    }

    pub fn run(&self, user: &str) -> Result<String, String> {
        chat(
            &self.client,
            &self.api_key,
            &self.model,
            &self.system,
            user,
            self.temperature,
        )
    }

    // One chat round-trip with an explicit message history and optional tool
    // specs. Returns the assistant's content and/or the tool calls it requested.
    pub fn complete(&self, messages: &[Json], tools: &[Json]) -> Result<Reply, String> {
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "temperature": self.temperature,
        });
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }

        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|e| format!("request failed: {}", e))?;

        let status = resp.status();
        let json: Json = resp
            .json()
            .map_err(|e| format!("invalid response body: {}", e))?;
        if !status.is_success() {
            let msg = json["error"]["message"].as_str().unwrap_or("unknown error");
            return Err(format!("OpenAI API error ({}): {}", status, msg));
        }

        let message = &json["choices"][0]["message"];
        let content = message["content"].as_str().map(|s| s.to_string());
        let mut tool_calls = Vec::new();
        if let Some(arr) = message["tool_calls"].as_array() {
            for tc in arr {
                tool_calls.push(ToolCall {
                    id: tc["id"].as_str().unwrap_or_default().to_string(),
                    name: tc["function"]["name"].as_str().unwrap_or_default().to_string(),
                    arguments: tc["function"]["arguments"]
                        .as_str()
                        .unwrap_or("{}")
                        .to_string(),
                });
            }
        }
        Ok(Reply {
            content,
            tool_calls,
        })
    }

    pub fn system(&self) -> &str {
        &self.system
    }

    // Run this agent over many inputs concurrently — one request per input.
    pub fn fan_out(&self, inputs: Vec<String>) -> Result<Vec<String>, String> {
        let mut handles = Vec::with_capacity(inputs.len());
        for input in inputs {
            let client = self.client.clone();
            let key = self.api_key.clone();
            let model = self.model.clone();
            let system = self.system.clone();
            let temp = self.temperature;
            handles.push(thread::spawn(move || {
                chat(&client, &key, &model, &system, &input, temp)
            }));
        }
        let mut out = Vec::with_capacity(handles.len());
        for h in handles {
            let res = h.join().map_err(|_| "worker thread panicked".to_string())?;
            out.push(res?);
        }
        Ok(out)
    }
}

fn chat(
    client: &Client,
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
    temperature: f64,
) -> Result<String, String> {
    let mut messages = Vec::new();
    if !system.is_empty() {
        messages.push(json!({ "role": "system", "content": system }));
    }
    messages.push(json!({ "role": "user", "content": user }));

    let body = json!({
        "model": model,
        "messages": messages,
        "temperature": temperature,
    });

    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .map_err(|e| format!("request failed: {}", e))?;

    let status = resp.status();
    let json: Json = resp
        .json()
        .map_err(|e| format!("invalid response body: {}", e))?;

    if !status.is_success() {
        let msg = json["error"]["message"]
            .as_str()
            .unwrap_or("unknown error");
        return Err(format!("OpenAI API error ({}): {}", status, msg));
    }

    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "no content in OpenAI response".to_string())
}
