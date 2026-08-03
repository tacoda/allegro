// A skill composes its instructions INTO an agent's system prompt (same call,
// same context) — distinct from a tool (code) or a delegate (another call).
//
//   allegro run examples/10_skill.ts --mock
//   OPENAI_API_KEY=sk-... allegro run examples/10_skill.ts

import { defineSystem } from "../src/index.ts";
import { mock } from "./_mock.ts";

export default defineSystem({
  nodes: {
    pirate: { type: "skill", description: "pirate voice", instructions: "Answer in pirate dialect, one short sentence." },
    bot: { type: "agent", system: "Be helpful.", uses: ["pirate"] },
  },
  transitions: { entry: "bot", bot: "end" },
  run: async (sys) => {
    mock(() => "Arr, the weather be fair, matey!");
    // The skill's instructions were folded into the agent's prompt:
    console.log("[system]", sys.agents.bot!.system.replace(/\n+/g, " "));
    console.log((await sys.run("How's the weather?")).content);
  },
});
