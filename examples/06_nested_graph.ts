// A graph node: a subgraph that runs as one flow step. "Everything is a graph"
// is recursive — a node can itself be nodes + transitions.
//
//   allegro run examples/06_nested_graph.ts   # -> "HI"

import { defineSystem } from "../src/index.ts";

export default defineSystem({
  nodes: {
    prep: { type: "fn", run: (msg) => msg.content.trim() },
    format: {
      type: "graph",
      nodes: {
        upper: { type: "fn", run: (msg) => msg.content.toUpperCase() },
        quote: { type: "fn", run: (msg) => `"${msg.content}"` },
      },
      transitions: { entry: "upper", upper: "quote", quote: "end" },
    },
  },
  transitions: { entry: "prep", prep: "format", format: "end" },
  run: async (sys) => {
    console.log((await sys.run("  hi  ")).content);
  },
});
