// Commands: named user-facing entrypoints into the graph. Each enters at a
// target node. Invoke with sys.command(name, input) or `--command`.
//
//   allegro run examples/05_commands.ts
//   allegro run examples/05_commands.ts --command length --input a,b,c,d   # -> 4

import { defineSystem } from "../src/index.ts";

export default defineSystem({
  nodes: {
    add: { type: "fn", run: (msg) => String(msg.content.split(",").reduce((a, b) => a + Number(b), 0)) },
    count: { type: "fn", run: (msg) => String(msg.content.split(",").length) },
  },
  transitions: { entry: "add", add: "end", count: "end" },
  commands: {
    sum: { target: "add", description: "Sum comma-separated numbers." },
    length: { target: "count", description: "Count comma-separated items." },
  },
  run: async (sys) => {
    console.log((await sys.command("sum", "1,2,3")).content); // 6
    console.log((await sys.command("length", "a,b,c,d")).content); // 4
  },
});
