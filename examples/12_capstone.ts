// The works: a routing desk that uses every edge kind. triage routes to a full
// desk agent (tool + skill + memory + a delegate) or to small talk; a preToolUse
// hook guards the tool; a command is the entrypoint.
//
//   allegro run examples/12_capstone.ts --mock
//   OPENAI_API_KEY=sk-... allegro run examples/12_capstone.ts --command ask --input "Please RESEARCH the moon."

import { defineSystem } from "../src/index.ts";
import { mockRaw, say, callTool, sawTool, lastToolOutput, systemText, firstUser } from "./_mock.ts";

export default defineSystem({
  nodes: {
    notes: { type: "memory" },
    wc: { type: "tool", description: "Count the words in the given text.", run: (t) => String(t.trim().split(/\s+/).filter(Boolean).length) },
    concise: { type: "skill", description: "brevity", instructions: "Answer in under 20 words." },
    researcher: { type: "agent", description: "Research a topic and return a short summary.", system: "Summarize the topic factually.", uses: ["concise"] },
    triage: { type: "agent", system: "Reply with exactly RESEARCH or CHAT." },
    desk: { type: "agent", system: "Answer the user. Count words and delegate research when useful.", uses: ["wc", "notes", "concise", "researcher"] },
    chitchat: { type: "agent", system: "Make friendly small talk.", uses: ["concise"] },
  },
  transitions: {
    entry: "triage",
    triage: (msg) => (msg.content.toUpperCase().includes("RESEARCH") ? "desk" : "chitchat"),
    desk: "end",
    chitchat: "end",
  },
  commands: {
    ask: { target: "triage", description: "Ask the desk; routes to research or chat." },
  },
  hooks: {
    preToolUse: { match: "wc", run: (e) => (e.input && e.input.trim() ? undefined : { block: true, reason: "empty text" }) },
    postToolUse: { run: (e) => void console.log(`[hook] ${e.tool} -> ${e.output}`) },
  },
  run: async (sys) => {
    // Mock: triage classifies; desk counts words then summarizes.
    mockRaw(async (p) => {
      if (/RESEARCH or CHAT/.test(systemText(p.messages))) {
        return say(/research/i.test(firstUser(p.messages)) ? "RESEARCH" : "CHAT");
      }
      if (!sawTool(p.messages)) return callTool("wc", firstUser(p.messages));
      return say(`Counted ${lastToolOutput(p.messages)} word(s); here is a brief note.`);
    });

    console.log("[desk wiring]", sys.agents.desk!.tools.map((t) => t.name).join(", "));
    console.log((await sys.command("ask", "Please RESEARCH the moon.")).content);
  },
});
