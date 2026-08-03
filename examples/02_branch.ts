// A conditional. A transition router is plain code that returns the next node —
// this is how branching lives in the graph, no LLM required.
//
//   allegro run examples/02_branch.ts       # -> even / odd

import { defineSystem } from "../src/index.ts";

export default defineSystem({
  nodes: {
    number: { type: "fn", run: (msg) => msg.content }, // pass the input through
    even: { type: "fn", run: () => "even" },
    odd: { type: "fn", run: () => "odd" },
  },
  transitions: {
    entry: "number",
    number: (msg) => (Number(msg.content) % 2 === 0 ? "even" : "odd"),
    even: "end",
    odd: "end",
  },
  run: async (sys) => {
    console.log((await sys.run("4")).content); // even
    console.log((await sys.run("7")).content); // odd
  },
});
