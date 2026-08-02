// A routing graph — classify, then answer only if it looks like math. An edge is
// a target node name or a router function returning the next node.
//
//   OPENAI_API_KEY=sk-... allegro run examples/triage.ts

import { defineSystem } from "../src/index.ts";

export default defineSystem({
  agents: {
    triage: { system: "Reply with exactly MATH or OTHER." },
    responder: { system: "Answer the question concisely." },
  },
  graphs: {
    desk: {
      entry: "classify",
      nodes: { classify: "triage", answer: "responder" },
      edges: {
        classify: (msg) => (msg.content.includes("MATH") ? "answer" : "end"),
        answer: "end",
      },
    },
  },
  run: async (sys) => {
    const out = await sys.graphs.desk!.trigger("What is 2 + 2?");
    console.log(out.content);
  },
});
