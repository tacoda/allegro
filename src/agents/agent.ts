import { chat, defaultModel } from "./openai.ts";
import { Message } from "./message.ts";
import { Memory } from "./memory.ts";
import { Model, Tool } from "./tool.ts";
import type { Subagent } from "./subagent.ts";
import { runtime, Runtime } from "../otp/runtime.ts";

export interface AgentConfig {
  name?: string;
  model?: string | Model;
  system?: string;
  temperature?: number;
  tools?: Tool[];
  memory?: Memory;
  subagents?: Subagent[];
}

const MAX_TOOL_STEPS = 8;

// An LLM plus the machinery around it: a system prompt, callable tools, an
// optional memory, and named subagents it can delegate to. `run` returns a
// Message; the run loops until the model stops calling tools.
export class Agent {
  name: string;
  model: string;
  temperature: number;
  system: string;
  tools: Tool[];
  memory?: Memory;
  subagents: Map<string, Subagent>;

  constructor(cfg: AgentConfig = {}, private rt: Runtime = runtime) {
    this.name = cfg.name ?? "agent";
    const model = cfg.model;
    this.model = typeof model === "string" ? model : (model?.name ?? defaultModel());
    this.temperature = cfg.temperature ?? (model instanceof Model ? model.temperature : 0.7);
    this.system = cfg.system ?? "";
    this.tools = cfg.tools ?? [];
    this.memory = cfg.memory;
    this.subagents = new Map((cfg.subagents ?? []).map((s) => [s.name, s]));
  }

  async run(input: string): Promise<Message> {
    this.rt.emit({ type: "agent", phase: "start", name: this.name, text: input });
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
        this.rt.emit({ type: "agent", phase: "finish", name: this.name, text: res.content });
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

  async delegate(name: string, input: string): Promise<Message> {
    const sub = this.subagents.get(name);
    if (!sub) throw new Error(`no subagent named '${name}'`);
    return sub.run(input);
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
    if (this.memory && name === "remember") {
      return this.memory.remember(String(args.key), String(args.value));
    }
    if (this.memory && name === "recall") {
      return this.memory.recall(String(args.key)) ?? "(not found)";
    }
    const tool = this.tools.find((t) => t.name === name);
    if (!tool) return `error: no tool '${name}'`;
    return tool.run(String(args.input ?? ""));
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
