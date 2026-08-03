// A routing graph between two agents: classify, then answer. A transition router
// decides the next node from the classifier's reply.
//
//   allegro run examples/09_triage.ts --mock       # -> 2 + 2 = 4.
//   OPENAI_API_KEY=sk-... allegro run examples/09_triage.ts

import { defineSystem } from "../src/index.ts";
import { mock } from "./_mock.ts";

const looksMathy = (s: string) => /\d/.test(s) && /[+\-*/]/.test(s);

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
    // Mock: the classifier says MATH; the responder answers a MATH hand-off.
    mock((user) => (user.trim() === "MATH" ? "2 + 2 = 4." : looksMathy(user) ? "MATH" : "OTHER"));
    console.log((await sys.run("What is 2 + 2?")).content);
  },
});
