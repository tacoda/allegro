// The smallest system: one `fn` node. No LLM, no edges beyond entry → end.
//
//   allegro run examples/01_hello.ts        # -> Hello, world!

import { defineSystem } from "../src/index.ts";

export default defineSystem({
  nodes: {
    greet: { type: "fn", run: (msg) => `Hello, ${msg.content}!` },
  },
  transitions: { entry: "greet", greet: "end" },
  run: async (sys) => {
    console.log((await sys.run("world")).content);
  },
});
