import { expect, test, beforeEach, afterEach } from "bun:test";
import { buildSystem, runSystem } from "../src/spec/index.ts";
import { defineSystem } from "../src/spec/index.ts";
import { GenServer, runtime } from "../src/otp/index.ts";
import { setChatBackend } from "../src/agents/index.ts";

beforeEach(() => runtime.reset());
afterEach(() => setChatBackend(null));

test("builds and wires tools, agents, and a graph", async () => {
  setChatBackend(async (p) => {
    const user = String((p.messages.at(-1) as any).content);
    return { message: { role: "assistant", content: user.toUpperCase() }, content: user.toUpperCase(), toolCalls: [] };
  });

  const sys = await buildSystem({
    tools: { shout: { description: "up", run: (i) => i.toUpperCase() } },
    agents: { bot: { system: "s", tools: ["shout"] } },
    graphs: {
      g: { entry: "a", nodes: { a: "bot" }, edges: { a: "end" } },
    },
  });

  expect(sys.tools.shout!.run("x")).toBe("X");
  expect((await sys.agents.bot!.run("hi")).content).toBe("HI");
  expect((await sys.graphs.g!.trigger("yo")).content).toBe("YO");
});

test("runs a GenServer + supervisor spec end to end", async () => {
  class Worker extends GenServer<number> {
    handleCall(msg: string, state: number) {
      if (msg === "crash") throw new Error("boom");
      return this.reply(state, state);
    }
  }

  let restarted = false;
  let finalId = -1;

  await runSystem(
    defineSystem({
      servers: { Worker },
      supervisors: { sup: { children: [{ server: Worker, args: [7] }] } },
      run: async (sys) => {
        const w = sys.supervisors.sup!.whichChildren()[0]!;
        await w.call("crash").catch(() => {});
        await new Promise((r) => setTimeout(r, 10));
        const w2 = sys.supervisors.sup!.whichChildren()[0]!;
        restarted = w.pid !== w2.pid;
        finalId = await w2.call("id");
      },
    }),
  );

  expect(restarted).toBe(true);
  expect(finalId).toBe(7);
});
