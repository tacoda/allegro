import type { Model, ToolFn, Tool, Memory, Agent, Subagent, Graph, GraphNode, GraphEdge } from "../agents/index.ts";
import type { GenServer, ServerRef, SupervisorRef, Runtime } from "../otp/index.ts";

// A GenServer subclass: constructible and startable.
export type ServerClass = (new () => GenServer) & {
  start(...args: any[]): Promise<ServerRef>;
};

export interface ToolSpec {
  description: string;
  run: ToolFn;
}

export interface AgentSpec {
  model?: string | Model;
  system?: string;
  temperature?: number;
  tools?: string[];
  subagents?: string[];
  memory?: string;
}

export interface SubagentSpec {
  description: string;
  model?: string | Model;
  system?: string;
  tools?: string[];
}

export interface GraphSpec {
  entry: string;
  nodes: Record<string, string | GraphNode>;
  edges: Record<string, GraphEdge>;
}

export interface SupChildSpec {
  server: ServerClass;
  args?: any[];
}

export interface SupervisorSpec {
  strategy?: "one_for_one";
  maxRestarts?: number;
  children: (ServerClass | SupChildSpec)[];
}

// The declarative shape. Structure is data; behavior (tool `run`, graph routers,
// GenServer callbacks) and `run` are inline TypeScript.
export interface SystemSpec {
  memory?: Record<string, Record<string, string>>;
  tools?: Record<string, ToolSpec>;
  subagents?: Record<string, SubagentSpec>;
  agents?: Record<string, AgentSpec>;
  servers?: Record<string, ServerClass>;
  graphs?: Record<string, GraphSpec>;
  supervisors?: Record<string, SupervisorSpec>;
  run?: (sys: System) => void | Promise<void>;
}

// The built system handed to `run` — every declared primitive, instantiated and
// wired, keyed by name.
export interface System {
  tools: Record<string, Tool>;
  memory: Record<string, Memory>;
  subagents: Record<string, Subagent>;
  agents: Record<string, Agent>;
  servers: Record<string, ServerClass>;
  graphs: Record<string, Graph>;
  supervisors: Record<string, SupervisorRef>;
  start(server: ServerClass, ...args: any[]): Promise<ServerRef>;
  runtime: Runtime;
}

export interface SystemDefinition {
  spec: SystemSpec;
}

// Author a system. The result is a definition; the CLI/TUI/web build and run it.
export function defineSystem(spec: SystemSpec): SystemDefinition {
  return { spec };
}
