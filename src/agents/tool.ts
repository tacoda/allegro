import { defaultModel } from "./openai.ts";

export type ToolFn = (input: string) => string | Promise<string>;

export interface ToolConfig {
  name?: string;
  description: string;
  run: ToolFn;
}

// A callable the model may invoke mid-run via function calling. `run` takes the
// tool's string input. Also callable directly.
export class Tool {
  name: string;
  description: string;
  run: ToolFn;

  constructor(cfg: ToolConfig) {
    this.name = cfg.name ?? "tool";
    this.description = cfg.description;
    this.run = cfg.run;
  }

  schema() {
    return {
      type: "function",
      function: {
        name: this.name,
        description: this.description,
        parameters: {
          type: "object",
          properties: { input: { type: "string", description: "the tool input" } },
          required: ["input"],
        },
      },
    };
  }
}

export class Model {
  constructor(
    public provider: string = "openai",
    public name: string = defaultModel(),
    public temperature: number = 0.7,
  ) {}
}
