// Native AI primitives — the low-level pieces (Model, Tool, Message) and the
// Agent runner, wired to the OpenAI backend. Higher-level concepts (Harness,
// Loop, Orchestrator, Supervisor) are allegro stdlib built on top of these.

use std::rc::Rc;

use serde_json::{json, Value as Json};

use crate::interp::Interp;
use crate::openai::{Agent as Core, ToolCall};
use crate::value::Value;

// Modules handled natively here (bare names act as the default alias for the
// `Allegro.*` namespace).
pub fn is_ai_module(module: &str) -> bool {
    matches!(
        strip(module),
        "Model" | "Tool" | "Agent" | "Message" | "Context" | "Memory"
    )
}

fn strip(module: &str) -> &str {
    module.strip_prefix("Allegro.").unwrap_or(module)
}

pub fn dispatch(
    interp: &mut Interp,
    module: &str,
    fun: &str,
    args: Vec<Value>,
) -> Result<Value, String> {
    match (strip(module), fun) {
        ("Model", "new") => Ok(model_new(&args)),
        ("Tool", "new") => tool_new(&args),
        ("Agent", "new") => Ok(agent_new(&args)),
        ("Agent", "run") => Ok(agent_run(interp, &args)),
        ("Agent", "run!") => agent_run_bang(interp, &args),
        ("Agent", "fan_out") => agent_fan_out(&args),
        ("Message", "new") => Ok(message_new(&args)),
        (m, f) => Err(format!("{}.{}/{} is undefined", m, f, args.len())),
    }
}

// ---- constructors ----

fn model_new(args: &[Value]) -> Value {
    let o = opts(args);
    tagged(
        "Model",
        vec![
            ("provider", Value::Str(get_str(&o, "provider", &default_provider()))),
            ("name", Value::Str(get_str(&o, "name", &default_model()))),
            ("temperature", Value::Float(get_num(&o, "temperature", 0.7))),
        ],
    )
}

fn tool_new(args: &[Value]) -> Result<Value, String> {
    let o = opts(args);
    let run = get(&o, "run").ok_or("Tool.new needs a `run:` function")?;
    Ok(tagged(
        "Tool",
        vec![
            ("name", Value::Str(get_str(&o, "name", "tool"))),
            ("description", Value::Str(get_str(&o, "description", ""))),
            ("run", run),
        ],
    ))
}

fn agent_new(args: &[Value]) -> Value {
    let o = opts(args);
    // `model:` may be a Model struct or a bare string; else env default.
    let (provider, model, model_temp) = match get(&o, "model") {
        Some(Value::Map(m)) => (
            field_str(&m, "provider", &default_provider()),
            field_str(&m, "name", &default_model()),
            field_num(&m, "temperature", 0.7),
        ),
        Some(Value::Str(s)) => (default_provider(), s, 0.7),
        _ => (default_provider(), default_model(), 0.7),
    };
    tagged(
        "Agent",
        vec![
            ("name", Value::Str(get_str(&o, "name", "agent"))),
            ("provider", Value::Str(provider)),
            ("model", Value::Str(model)),
            ("system", Value::Str(get_str(&o, "system", ""))),
            ("temperature", Value::Float(get_num(&o, "temperature", model_temp))),
            ("tools", get(&o, "tools").unwrap_or_else(|| Value::list(vec![]))),
        ],
    )
}

fn message_new(args: &[Value]) -> Value {
    let content = args.first().map(|v| v.to_string()).unwrap_or_default();
    let from = args.get(1).map(|v| v.to_string()).unwrap_or_else(|| "user".into());
    message(content, &from)
}

// ---- Agent.run ----

fn agent_run(interp: &mut Interp, args: &[Value]) -> Value {
    // data-first: run(input, agent)
    let input = args.first().map(|v| v.to_string()).unwrap_or_default();
    let agent = match args.get(1) {
        Some(Value::Map(m)) => m.clone(),
        _ => return err_tuple("Agent.run expects an agent as its second argument"),
    };
    let name = field_str(&agent, "name", "agent");
    let core = match Core::new(
        &field_str(&agent, "provider", "openai"),
        field_str(&agent, "model", &default_model()),
        field_str(&agent, "system", ""),
        field_num(&agent, "temperature", 0.7),
    ) {
        Ok(c) => c,
        Err(e) => return err_tuple(&e),
    };
    let tools = field(&agent, "tools").unwrap_or_else(|| Value::list(vec![]));
    let result = match as_list(&tools) {
        list if list.is_empty() => core.run(&input),
        list => run_tool_loop(interp, &core, &list, &input),
    };
    match result {
        Ok(content) => ok_tuple(message(content, &name)),
        Err(e) => err_tuple(&e),
    }
}

fn agent_run_bang(interp: &mut Interp, args: &[Value]) -> Result<Value, String> {
    match agent_run(interp, args) {
        Value::Tuple(t) if is_tag(&t, "ok") => Ok(t[1].clone()),
        Value::Tuple(t) if is_tag(&t, "error") => Err(t[1].to_string()),
        other => Ok(other),
    }
}

fn agent_fan_out(args: &[Value]) -> Result<Value, String> {
    let inputs: Vec<String> = match args.first() {
        Some(Value::List(l)) => l.iter().map(|v| v.to_string()).collect(),
        _ => return Err("Agent.fan_out expects a list of inputs".into()),
    };
    let agent = match args.get(1) {
        Some(Value::Map(m)) => m.clone(),
        _ => return Err("Agent.fan_out expects an agent".into()),
    };
    let name = field_str(&agent, "name", "agent");
    let core = Core::new(
        &field_str(&agent, "provider", "openai"),
        field_str(&agent, "model", &default_model()),
        field_str(&agent, "system", ""),
        field_num(&agent, "temperature", 0.7),
    )?;
    let outs = core.fan_out(inputs)?;
    Ok(Value::list(outs.into_iter().map(|c| message(c, &name)).collect()))
}

// The tool-calling loop: complete → run any requested tools → feed results back
// → repeat until the model returns a final answer.
fn run_tool_loop(
    interp: &mut Interp,
    core: &Core,
    tools: &[Value],
    input: &str,
) -> Result<String, String> {
    let specs = tool_specs(tools);
    let mut messages: Vec<Json> = Vec::new();
    if !core.system().is_empty() {
        messages.push(json!({"role": "system", "content": core.system()}));
    }
    messages.push(json!({"role": "user", "content": input}));

    for _ in 0..8 {
        let reply = core.complete(&messages, &specs)?;
        if reply.tool_calls.is_empty() {
            return Ok(reply.content.unwrap_or_default());
        }
        messages.push(assistant_calls(&reply.tool_calls));
        for tc in &reply.tool_calls {
            let out = run_tool(interp, tools, tc)?;
            messages.push(json!({"role": "tool", "tool_call_id": tc.id, "content": out}));
        }
    }
    Err("tool loop exceeded 8 rounds without a final answer".into())
}

fn run_tool(interp: &mut Interp, tools: &[Value], tc: &ToolCall) -> Result<String, String> {
    let tool = tools
        .iter()
        .find_map(|t| match t {
            Value::Map(m) if field_str(m, "name", "") == tc.name => Some(m.clone()),
            _ => None,
        })
        .ok_or_else(|| format!("the model called an unknown tool '{}'", tc.name))?;
    let run = field(&tool, "run").ok_or("tool has no run function")?;
    let arg = tool_input(&tc.arguments);
    match run {
        Value::Fun(f) => Ok(interp.call_fun(&f, vec![Value::Str(arg)])?.to_string()),
        _ => Err("tool run must be a function".into()),
    }
}

// The model sends `{"input": "..."}`; pass the `input` field, else the raw JSON.
fn tool_input(arguments: &str) -> String {
    serde_json::from_str::<Json>(arguments)
        .ok()
        .and_then(|j| j.get("input").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| arguments.to_string())
}

fn tool_specs(tools: &[Value]) -> Vec<Json> {
    tools
        .iter()
        .filter_map(|t| match t {
            Value::Map(m) => Some(json!({
                "type": "function",
                "function": {
                    "name": field_str(m, "name", "tool"),
                    "description": field_str(m, "description", ""),
                    "parameters": {
                        "type": "object",
                        "properties": { "input": { "type": "string", "description": "tool input" } },
                        "required": ["input"]
                    }
                }
            })),
            _ => None,
        })
        .collect()
}

fn assistant_calls(calls: &[ToolCall]) -> Json {
    let tcs: Vec<Json> = calls
        .iter()
        .map(|tc| {
            json!({
                "id": tc.id,
                "type": "function",
                "function": { "name": tc.name, "arguments": tc.arguments }
            })
        })
        .collect();
    json!({"role": "assistant", "content": null, "tool_calls": tcs})
}

// ---- value helpers ----

type Pairs = Vec<(String, Value)>;

fn opts(args: &[Value]) -> Pairs {
    match args.first() {
        Some(Value::List(items)) => items
            .iter()
            .filter_map(|it| match it {
                Value::Tuple(t) if t.len() == 2 => match &t[0] {
                    Value::Atom(k) => Some((k.clone(), t[1].clone())),
                    _ => None,
                },
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn get(pairs: &Pairs, key: &str) -> Option<Value> {
    pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

fn get_str(pairs: &Pairs, key: &str, default: &str) -> String {
    get(pairs, key).map(|v| v.to_string()).unwrap_or_else(|| default.to_string())
}

fn get_num(pairs: &Pairs, key: &str, default: f64) -> f64 {
    match get(pairs, key) {
        Some(Value::Int(n)) => n as f64,
        Some(Value::Float(f)) => f,
        _ => default,
    }
}

fn tagged(tag: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut out: Vec<(Value, Value)> =
        vec![(Value::Atom("__struct__".into()), Value::Atom(tag.into()))];
    for (k, v) in fields {
        out.push((Value::Atom(k.into()), v));
    }
    Value::Map(Rc::new(out))
}

fn field(map: &[(Value, Value)], key: &str) -> Option<Value> {
    Value::map_get(map, &Value::Atom(key.into()))
}

fn field_str(map: &[(Value, Value)], key: &str, default: &str) -> String {
    field(map, key).map(|v| v.to_string()).unwrap_or_else(|| default.to_string())
}

fn field_num(map: &[(Value, Value)], key: &str, default: f64) -> f64 {
    match field(map, key) {
        Some(Value::Int(n)) => n as f64,
        Some(Value::Float(f)) => f,
        _ => default,
    }
}

fn as_list(v: &Value) -> Vec<Value> {
    match v {
        Value::List(l) => (**l).clone(),
        _ => Vec::new(),
    }
}

fn message(content: String, from: &str) -> Value {
    tagged(
        "Message",
        vec![
            ("content", Value::Str(content)),
            ("role", Value::Str("assistant".into())),
            ("from", Value::Str(from.into())),
        ],
    )
}

fn ok_tuple(v: Value) -> Value {
    Value::tuple(vec![Value::Atom("ok".into()), v])
}

fn err_tuple(reason: &str) -> Value {
    Value::tuple(vec![Value::Atom("error".into()), Value::Str(reason.into())])
}

fn is_tag(t: &[Value], tag: &str) -> bool {
    matches!(t.first(), Some(Value::Atom(a)) if a == tag) && t.len() == 2
}

fn default_model() -> String {
    std::env::var("MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string())
}

fn default_provider() -> String {
    std::env::var("PROVIDER").unwrap_or_else(|_| "openai".to_string())
}
