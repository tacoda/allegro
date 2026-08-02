// An agent with a tool and memory. The tool body is an inline function; memory
// gives the model built-in remember/recall tools.
//
//   OPENAI_API_KEY=sk-... allegro run examples/assistant.ts

import { defineSystem } from "../src/index.ts";

export default defineSystem({
  memory: { notes: {} },
  tools: {
    shout: { description: "Convert text to UPPERCASE. Use when asked to shout.", run: (input) => input.toUpperCase() },
  },
  agents: {
    bot: {
      system: "Use your tools. Remember facts and recall them before answering.",
      tools: ["shout"],
      memory: "notes",
    },
  },
  run: async (sys) => {
    console.log((await sys.agents.bot!.run("Please shout: hello")).content);
    await sys.agents.bot!.run("My favorite color is teal.");
    console.log((await sys.agents.bot!.run("What is my favorite color?")).content);
  },
});
