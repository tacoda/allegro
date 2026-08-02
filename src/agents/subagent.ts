import { Agent, type AgentConfig } from "./agent.ts";
import type { Message } from "./message.ts";

export interface SubagentConfig extends AgentConfig {
  name: string;
  description: string;
}

// A named, described worker an agent delegates to.
export class Subagent {
  name: string;
  description: string;
  agent: Agent;

  constructor(cfg: SubagentConfig) {
    this.name = cfg.name;
    this.description = cfg.description;
    this.agent = new Agent({ ...cfg, name: cfg.name });
  }

  run(input: string): Promise<Message> {
    return this.agent.run(input);
  }
}
