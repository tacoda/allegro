// A routing graph — classify, then answer only if it looks like math. A
// transition is a target node name or a router (deterministic code) returning
// the next node; `"end"` stops.
//
//   OPENAI_API_KEY=sk-... allegro run examples/triage.ts

import { defineSystem } from "../src/index.ts";

export default defineSystem({
  nodes: {
    triage: { type: "agent", system: "Reply with exactly MATH or OTHER." },
    responder: { type: "agent", system: "Answer the question concisely." },
  },
  transitions: {
    entry: "triage",
    triage: (msg) => (msg.content.includes("MATH") ? "responder" : "end"),
    responder: "end",
  },
  run: async (sys) => {
    console.log((await sys.run("What is 2 + 2?")).content);
  },
});
