import { Agent } from "./agent.ts";
import { Message } from "./message.ts";
import { bus } from "../runtime/bus.ts";

// A runtime node: an agent (LLM), a deterministic fn (code), or a nested graph.
export type GraphNode =
  | Agent
  | Graph
  | ((msg: Message) => Message | string | Promise<Message | string>);

// An edge is a target node name, or a router (deterministic code) that returns
// the next name. This is where conditionals live.
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

  async trigger(input: string, from: string = this.entry): Promise<Message> {
    let name = from;
    let msg = new Message(input, "user", "user");
    for (let step = 0; step < this.maxSteps; step++) {
      const node = this.nodes[name];
      if (!node) throw new Error(`graph has no node '${name}'`);
      bus.emit({ type: "nodeEnter", agent: name });
      msg = await runNode(node, msg);
      bus.emit({ type: "nodeExit", agent: name });

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
  if (node instanceof Agent) return node.run(msg.content);
  if (node instanceof Graph) return node.trigger(msg.content);
  const out = await node(msg);
  return typeof out === "string" ? new Message(out, "assistant", "node") : out;
}
