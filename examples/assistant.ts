// An agent with a tool and memory. `uses` lists its dependencies by name; a
// memory node gives the model built-in remember/recall tools.
//
//   OPENAI_API_KEY=sk-... allegro run examples/assistant.ts

import { defineSystem } from "../src/index.ts";

export default defineSystem({
  nodes: {
    notes: { type: "memory" },
    shout: {
      type: "tool",
      description: "Convert text to UPPERCASE. Use when asked to shout.",
      run: (input) => input.toUpperCase(),
    },
    bot: {
      type: "agent",
      system: "Use your tools. Remember facts and recall them before answering.",
      uses: ["shout", "notes"],
    },
  },
  transitions: { entry: "bot", bot: "end" },
  run: async (sys) => {
    console.log((await sys.agents.bot!.run("Please shout: hello")).content);
    await sys.agents.bot!.run("My favorite color is teal.");
    console.log((await sys.agents.bot!.run("What is my favorite color?")).content);
  },
});
