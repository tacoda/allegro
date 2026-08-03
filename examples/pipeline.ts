// Deterministic control flow — no LLM. `fn` nodes do the work, a router
// transition branches on the message, a hook observes stop, and a command adds
// a user-facing entrypoint. Runs offline.
//
//   allegro run examples/pipeline.ts
//   allegro run examples/pipeline.ts --command measure --input abcdef

import { defineSystem } from "../src/index.ts";

export default defineSystem({
  nodes: {
    parse: { type: "fn", run: (m) => String(m.content.trim().length) },
    big: { type: "fn", run: () => "big" },
    small: { type: "fn", run: () => "small" },
  },
  transitions: {
    entry: "parse",
    parse: (m) => (Number(m.content) > 5 ? "big" : "small"),
    big: "end",
    small: "end",
  },
  commands: {
    measure: { target: "parse", description: "Report whether input is big or small." },
  },
  hooks: {
    stop: { run: () => void console.log("(done)") },
  },
  run: async (sys) => {
    console.log((await sys.run("hello world")).content); // 11 chars -> big
    console.log((await sys.run("hi")).content); // 2 chars -> small
  },
});
