// A loop. A transition can route back to an earlier node; the graph steps until
// the router returns "end". Deterministic countdown, no LLM.
//
//   allegro run examples/03_loop.ts         # -> 0

import { defineSystem } from "../src/index.ts";

export default defineSystem({
  nodes: {
    tick: { type: "fn", run: (msg) => String(Number(msg.content) - 1) },
  },
  transitions: {
    entry: "tick",
    tick: (msg) => (Number(msg.content) > 0 ? "tick" : "end"),
  },
  run: async (sys) => {
    console.log((await sys.run("3")).content); // 3 -> 2 -> 1 -> 0, then stop
  },
});
