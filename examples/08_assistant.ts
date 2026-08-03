// An agent with a tool and memory. `uses` names its dependencies; memory adds
// built-in remember/recall tools. Runs mocked or with a real key.
//
//   allegro run examples/08_assistant.ts --mock
//   OPENAI_API_KEY=sk-... allegro run examples/08_assistant.ts

import { defineSystem } from "../src/index.ts";
import { mockRaw, say, callTool, sawTool, lastToolOutput } from "./_mock.ts";

export default defineSystem({
  nodes: {
    notes: { type: "memory" },
    shout: { type: "tool", description: "Convert text to UPPERCASE.", run: (input) => input.toUpperCase() },
    bot: { type: "agent", system: "Use your tools; be brief.", uses: ["shout", "notes"] },
  },
  transitions: { entry: "bot", bot: "end" },
  run: async (sys) => {
    // Mock: call the shout tool, then report its output.
    mockRaw(async (p) => (sawTool(p.messages) ? say(`Done: ${lastToolOutput(p.messages)}`) : callTool("shout", "hello")));
    console.log((await sys.run("Please shout hello")).content);
  },
});
