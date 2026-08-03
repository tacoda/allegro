import { chat, defaultModel } from "./openai.ts";
import { Message } from "./message.ts";
import { Memory } from "./memory.ts";
import { Model, Tool } from "./tool.ts";
import { bus } from "../runtime/bus.ts";

export interface AgentConfig {
  name?: string;
  description?: string; // present => delegatable by other agents
  model?: string | Model;
  system?: string;
  temperature?: number;
  tools?: Tool[];
  memory?: Memory;
}

const MAX_TOOL_STEPS = 8;

// An LLM plus its system prompt, callable tools, and optional memory. Skills and
// delegated agents are folded into `system`/`tools` at build time, so the agent
// itself only ever sees tools + a prompt. `run` loops until the model stops
// calling tools. Tool calls pass through preToolUse/postToolUse hooks.
export class Agent {
  name: string;
  description?: string;
  model: string;
  temperature: number;
  system: string;
  tools: Tool[];
  memory?: Memory;

  constructor(cfg: AgentConfig = {}) {
    this.name = cfg.name ?? "agent";
    this.description = cfg.description;
    const model = cfg.model;
    this.model = typeof model === "string" ? model : (model?.name ?? defaultModel());
    this.temperature = cfg.temperature ?? (model instanceof Model ? model.temperature : 0.7);
    this.system = cfg.system ?? "";
    this.tools = cfg.tools ?? [];
    this.memory = cfg.memory;
  }

  async run(input: string): Promise<Message> {
    await bus.fire("agentStart", { agent: this.name, input });
    const messages: any[] = [];
    if (this.system) messages.push({ role: "system", content: this.system });
    messages.push({ role: "user", content: input });

    const schemas = this.toolSchemas();
    for (let step = 0; step < MAX_TOOL_STEPS; step++) {
      const res = await chat({
        model: this.model,
        temperature: this.temperature,
        messages,
        tools: schemas.length ? schemas : undefined,
      });
      if (res.toolCalls.length === 0) {
        await bus.fire("agentFinish", { agent: this.name, output: res.content });
        return new Message(res.content, "assistant", this.name);
      }
      messages.push(res.message);
      for (const call of res.toolCalls) {
        const result = await this.invokeTool(call.name, call.args);
        messages.push({ role: "tool", tool_call_id: call.id, content: result });
      }
    }
    const last = messages[messages.length - 1]?.content ?? "";
    return new Message(String(last), "assistant", this.name);
  }

  ask(input: string): Promise<Message> {
    return this.run(input);
  }

  // Run over many inputs concurrently (real network parallelism), in order.
  fanOut(inputs: string[]): Promise<Message[]> {
    return Promise.all(inputs.map((i) => this.run(i)));
  }

  private toolSchemas(): any[] {
    const schemas: any[] = this.tools.map((t) => t.schema());
    if (this.memory) {
      schemas.push(memoryToolSchema("remember", "Store a fact as key/value for later."));
      schemas.push(memoryToolSchema("recall", "Look up a previously remembered fact by key."));
    }
    return schemas;
  }

  private async invokeTool(name: string, args: any): Promise<string> {
    const input = String(args.input ?? "");
    const decision = await bus.fire("preToolUse", { agent: this.name, tool: name, input });
    if (decision && "block" in decision) return `blocked: ${decision.reason ?? "denied by hook"}`;
    const effective = decision && "replace" in decision ? decision.replace : input;

    const output = await this.dispatchTool(name, args, effective);
    await bus.fire("postToolUse", { agent: this.name, tool: name, input: effective, output });
    return output;
  }

  private async dispatchTool(name: string, args: any, input: string): Promise<string> {
    if (this.memory && name === "remember") {
      return this.memory.remember(String(args.key), String(args.value));
    }
    if (this.memory && name === "recall") {
      return this.memory.recall(String(args.key)) ?? "(not found)";
    }
    const tool = this.tools.find((t) => t.name === name);
    if (!tool) return `error: no tool '${name}'`;
    return tool.run(input);
  }
}

function memoryToolSchema(name: string, description: string) {
  const properties =
    name === "remember"
      ? { key: { type: "string" }, value: { type: "string" } }
      : { key: { type: "string" } };
  const required = name === "remember" ? ["key", "value"] : ["key"];
  return { type: "function", function: { name, description, parameters: { type: "object", properties, required } } };
}
