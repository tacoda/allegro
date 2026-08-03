// A pipeline: fn nodes chained by transitions, each transforming the message.
// Deterministic, no LLM.
//
//   allegro run examples/04_pipeline.ts     # -> HELLO!

import { defineSystem } from "../src/index.ts";

export default defineSystem({
  nodes: {
    trim: { type: "fn", run: (msg) => msg.content.trim() },
    upper: { type: "fn", run: (msg) => msg.content.toUpperCase() },
    bang: { type: "fn", run: (msg) => `${msg.content}!` },
  },
  transitions: { entry: "trim", trim: "upper", upper: "bang", bang: "end" },
  run: async (sys) => {
    console.log((await sys.run("  hello  ")).content);
  },
});
