import { expect, test, beforeEach, afterEach } from "bun:test";
import { buildSystem } from "../src/spec/index.ts";
import { setChatBackend } from "../src/agents/index.ts";
import { bus } from "../src/runtime/bus.ts";

beforeEach(() => bus.reset());
afterEach(() => setChatBackend(null));

// A backend that echoes the last user message uppercased, calling no tools.
function echoUpper() {
  setChatBackend(async (p) => {
    const user = String((p.messages.at(-1) as any).content);
    const content = user.toUpperCase();
    return { message: { role: "assistant", content }, content, toolCalls: [] };
  });
}

test("wires tool + memory into an agent via uses; runs the root graph", async () => {
  echoUpper();
  const sys = await buildSystem({
    nodes: {
      shout: { type: "tool", description: "up", run: (i) => i.toUpperCase() },
      notes: { type: "memory" },
      bot: { type: "agent", system: "s", uses: ["shout", "notes"] },
    },
    transitions: { entry: "bot", bot: "end" },
  });

  expect(sys.tools.shout!.run("x")).toBe("X");
  expect((await sys.agents.bot!.run("hi")).content).toBe("HI");
  expect((await sys.run("yo")).content).toBe("YO");
});

test("fn nodes + conditional transitions run offline", async () => {
  const sys = await buildSystem({
    nodes: {
      parse: { type: "fn", run: (m) => String(m.content.length) },
      big: { type: "fn", run: () => "big" },
      small: { type: "fn", run: () => "small" },
    },
    transitions: {
      entry: "parse",
      parse: (m) => (Number(m.content) > 3 ? "big" : "small"),
      big: "end",
      small: "end",
    },
  });

  expect((await sys.run("hello")).content).toBe("big");
  expect((await sys.run("hi")).content).toBe("small");
});

test("skill instructions inject into the agent's system prompt", async () => {
  echoUpper();
  const sys = await buildSystem({
    nodes: {
      poet: { type: "skill", description: "poetry", instructions: "Write in verse." },
      bard: { type: "agent", system: "Base.", uses: ["poet"] },
    },
    transitions: { entry: "bard", bard: "end" },
  });

  expect(sys.agents.bard!.system).toContain("Write in verse.");
  expect(sys.agents.bard!.system).toContain("Base.");
});

test("an agent used by another becomes a delegation tool", async () => {
  echoUpper();
  const sys = await buildSystem({
    nodes: {
      worker: { type: "agent", description: "does work", system: "w" },
      boss: { type: "agent", system: "b", uses: ["worker"] },
    },
    transitions: { entry: "boss", boss: "end" },
  });

  expect(sys.agents.boss!.tools.map((t) => t.name)).toContain("worker");
});

test("a nested graph node runs as a single flow step", async () => {
  const sys = await buildSystem({
    nodes: {
      inner: {
        type: "graph",
        nodes: { step: { type: "fn", run: (m) => `${m.content}!` } },
        transitions: { entry: "step", step: "end" },
      },
    },
    transitions: { entry: "inner", inner: "end" },
  });

  expect((await sys.run("hi")).content).toBe("hi!");
});

test("a preToolUse hook can block a tool call", async () => {
  let called = 0;
  setChatBackend(async (p) => {
    const sawTool = p.messages.some((m: any) => m.role === "tool");
    if (!sawTool) {
      return {
        message: { role: "assistant", content: null },
        content: "",
        toolCalls: [{ id: "1", name: "danger", args: { input: "x" } }],
      };
    }
    const toolMsg: any = p.messages.find((m: any) => m.role === "tool");
    return { message: { role: "assistant", content: String(toolMsg.content) }, content: String(toolMsg.content), toolCalls: [] };
  });

  const sys = await buildSystem({
    nodes: {
      danger: {
        type: "tool",
        description: "d",
        run: () => {
          called++;
          return "ran";
        },
      },
      bot: { type: "agent", system: "s", uses: ["danger"] },
    },
    transitions: { entry: "bot", bot: "end" },
    hooks: { preToolUse: { match: "danger", run: () => ({ block: true, reason: "nope" }) } },
  });

  const out = await sys.agents.bot!.run("go");
  expect(called).toBe(0);
  expect(out.content).toContain("blocked");
});

test("a command enters the graph at its target node", async () => {
  const sys = await buildSystem({
    nodes: { echo: { type: "fn", run: (m) => `got:${m.content}` } },
    transitions: { entry: "echo", echo: "end" },
    commands: { go: { target: "echo" } },
  });

  expect((await sys.command("go", "hey")).content).toBe("got:hey");
});
