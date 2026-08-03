import type { Model, ToolFn, Tool, Memory, Agent, Graph } from "../agents/index.ts";
import type { Message } from "../agents/message.ts";

// ── Nodes (graph vertices) ──────────────────────────────────────────────────
// Every primitive is a node with a `type`. References between nodes are by name.

export interface ToolNode {
  type: "tool";
  description: string;
  run: ToolFn; // (input: string) => string | Promise<string>
}

// A deterministic flow step: plain code, no LLM. Returns the message to pass on.
export interface FnNode {
  type: "fn";
  run: (msg: Message) => Message | string | Promise<Message | string>;
}

export interface MemoryNode {
  type: "memory";
  seed?: Record<string, string>;
}

// An instruction bundle composed INTO an agent (same call, same context). `uses`
// names tools the skill brings with it.
export interface SkillNode {
  type: "skill";
  description: string;
  instructions: string;
  uses?: string[];
}

// An LLM actor. `description` makes it delegatable — an agent listed in another
// agent's `uses` is exposed to it as a callable (its own call, its own context).
// `uses` names tools | fns | skills | agents | mcp | memory.
export interface AgentNode {
  type: "agent";
  description?: string;
  model?: string | Model;
  system?: string;
  temperature?: number;
  uses?: string[];
}

// An external MCP server. Expands into callable tools an agent may `use`.
export interface McpNode {
  type: "mcp";
  server: string; // stdio launch command, e.g. "npx -y @modelcontextprotocol/server-github"
  env?: Record<string, string>;
  tools?: string[]; // allowlist; omit for all the server exposes
}

// A composite: its own nodes + transitions. Recursive — the system root is one.
export interface GraphNode {
  type: "graph";
  nodes: Record<string, SpecNode>;
  transitions: Transitions;
}

export type SpecNode = ToolNode | FnNode | MemoryNode | SkillNode | AgentNode | McpNode | GraphNode;

// ── Edges ───────────────────────────────────────────────────────────────────

// A transition is a target node name, or a router that returns the next name.
// A router is deterministic code — this is how conditionals live in the graph.
// `"end"` (or a missing transition) stops the run.
export type Transition = string | ((msg: Message) => string | Promise<string>);

// Flow edges over the sibling nodes. `entry` names the first node to run.
export interface Transitions {
  entry: string;
  [node: string]: Transition;
}

// ── Triggers (edges fired from outside the flow) ────────────────────────────

// A user-facing entrypoint into the graph.
export interface CommandSpec {
  target: string; // node name to run
  description?: string;
  input?: string; // preset/prefix input
}

export type HookEvent =
  | "sessionStart"
  | "userPromptSubmit"
  | "preToolUse"
  | "postToolUse"
  | "agentStart"
  | "agentFinish"
  | "stop";

export interface HookPayload {
  event: HookEvent;
  tool?: string;
  agent?: string;
  input?: string;
  output?: string;
  text?: string;
}

// A hook may observe, block (deny), or replace the input of the gated action.
// Only `preToolUse` acts on block/replace; other events ignore the result.
export type HookResult = void | { block: true; reason?: string } | { replace: string };

export interface Hook {
  match?: string; // e.g. tool name for pre/postToolUse; substring match
  run: (ev: HookPayload) => HookResult | Promise<HookResult>;
}

// ── System ──────────────────────────────────────────────────────────────────

// The declarative shape. Structure is data; behavior (tool/fn `run`, transition
// routers, hook handlers, `run`) is inline TypeScript.
export interface SystemSpec {
  nodes: Record<string, SpecNode>;
  transitions: Transitions;
  commands?: Record<string, CommandSpec>;
  hooks?: Partial<Record<HookEvent, Hook | Hook[]>>;
  run?: (sys: System) => void | Promise<void>;
}

// The built system handed to `run`: typed views over the instantiated nodes plus
// the root graph.
export interface System {
  nodes: Record<string, unknown>;
  tools: Record<string, Tool>;
  memory: Record<string, Memory>;
  agents: Record<string, Agent>;
  graphs: Record<string, Graph>;
  graph: Graph; // the root
  run(input: string): Promise<Message>; // trigger the root graph
  command(name: string, input?: string): Promise<Message>;
}

export interface SystemDefinition {
  spec: SystemSpec;
}

// Author a system. The result is a definition; the CLI/TUI/web build and run it.
export function defineSystem(spec: SystemSpec): SystemDefinition {
  return { spec };
}
