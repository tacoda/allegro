// Hooks fire on lifecycle events. These observe the run; a preToolUse hook could
// also block or replace a tool call (see 12_capstone). Offline.
//
//   allegro run examples/07_lifecycle_hooks.ts

import { defineSystem } from "../src/index.ts";

export default defineSystem({
  nodes: {
    work: { type: "fn", run: (msg) => `did: ${msg.content}` },
  },
  transitions: { entry: "work", work: "end" },
  hooks: {
    sessionStart: { run: () => void console.log("[hook] session start") },
    stop: { run: () => void console.log("[hook] stop") },
  },
  run: async (sys) => {
    console.log((await sys.run("task")).content);
  },
});
