import { Agent } from "./agent.ts";
import { Subagent } from "./subagent.ts";
import { Message } from "./message.ts";

export type GraphNode =
  | Agent
  | Subagent
  | ((msg: Message) => Message | string | Promise<Message | string>);

// An edge is a target node name or a router that returns the next name.
export type GraphEdge = string | ((msg: Message) => string | Promise<string>);

export interface GraphConfig {
  entry: string;
  nodes: Record<string, GraphNode>;
  edges: Record<string, GraphEdge>;
  maxSteps?: number;
}

// Control-flow routing over nodes. Each node's output feeds the next; an edge
// resolves the next node name, and "end" (or a missing edge) stops.
export class Graph {
  entry: string;
  nodes: Record<string, GraphNode>;
  edges: Record<string, GraphEdge>;
  maxSteps: number;

  constructor(cfg: GraphConfig) {
    this.entry = cfg.entry;
    this.nodes = cfg.nodes;
    this.edges = cfg.edges;
    this.maxSteps = cfg.maxSteps ?? 100;
  }

  async trigger(input: string): Promise<Message> {
    let name = this.entry;
    let msg = new Message(input, "user", "user");
    for (let step = 0; step < this.maxSteps; step++) {
      const node = this.nodes[name];
      if (!node) throw new Error(`graph has no node '${name}'`);
      msg = await runNode(node, msg);

      const edge = this.edges[name];
      const next = typeof edge === "function" ? await edge(msg) : edge;
      if (!next || next === "end") return msg;
      name = next;
    }
    return msg;
  }

  run(input: string): Promise<Message> {
    return this.trigger(input);
  }
}

async function runNode(node: GraphNode, msg: Message): Promise<Message> {
  if (node instanceof Agent || node instanceof Subagent) return node.run(msg.content);
  const out = await node(msg);
  return typeof out === "string" ? new Message(out, "assistant", "node") : out;
}
