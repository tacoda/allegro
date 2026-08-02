import { expect, test, afterEach } from "bun:test";
import {
  Agent,
  Tool,
  Memory,
  Graph,
  Message,
  setChatBackend,
  type ChatParams,
  type ChatResult,
} from "../src/agents/index.ts";

afterEach(() => setChatBackend(null));

// A scripted backend: each call consumes the next turn (a ChatResult or a
// function of the params).
type Turn = ChatResult | ((p: ChatParams) => ChatResult);
function script(turns: Turn[]) {
  let i = 0;
  setChatBackend(async (p) => {
    const turn = turns[Math.min(i++, turns.length - 1)]!;
    return typeof turn === "function" ? turn(p) : turn;
  });
}

const say = (content: string): ChatResult => ({
  message: { role: "assistant", content },
  content,
  toolCalls: [],
});

const callTool = (name: string, args: any): ChatResult => ({
  message: {
    role: "assistant",
    content: "",
    tool_calls: [{ id: "c1", type: "function", function: { name, arguments: JSON.stringify(args) } }],
  },
  content: "",
  toolCalls: [{ id: "c1", name, args }],
});

test("agent returns a Message", async () => {
  script([say("Paris")]);
  const bot = new Agent({ system: "terse" });
  const reply = await bot.run("capital of France?");
  expect(reply).toBeInstanceOf(Message);
  expect(reply.content).toBe("Paris");
});

test("agent runs the tool-calling loop", async () => {
  const shout = new Tool({ name: "shout", description: "uppercase", run: (t) => t.toUpperCase() });
  // turn 1: model calls the tool; turn 2: model answers with the result
  script([callTool("shout", { input: "hello" }), (p) => say((p.messages.at(-1) as any).content)]);
  const bot = new Agent({ tools: [shout] });
  const reply = await bot.run("shout hello");
  expect(reply.content).toBe("HELLO");
});

test("memory remember/recall tools work", async () => {
  const notes = new Memory();
  script([
    callTool("remember", { key: "color", value: "teal" }),
    () => say("stored"),
    callTool("recall", { key: "color" }),
    (p) => say((p.messages.at(-1) as any).content),
  ]);
  const bot = new Agent({ memory: notes });
  await bot.run("my color is teal");
  const reply = await bot.run("what is my color?");
  expect(reply.content).toBe("teal");
  expect(notes.recall("color")).toBe("teal");
});

test("fanOut runs inputs concurrently, in order", async () => {
  setChatBackend(async (p) => say(String((p.messages.at(-1) as any).content).toUpperCase()));
  const bot = new Agent();
  const out = await bot.fanOut(["a", "b", "c"]);
  expect(out.map((m) => m.content)).toEqual(["A", "B", "C"]);
});

test("graph routes to a node or ends", async () => {
  setChatBackend(async (p) => {
    const user = String((p.messages.at(-1) as any).content);
    return say(user.includes("2 + 2") ? "MATH" : "4");
  });
  const triage = new Agent({ system: "classify" });
  const answer = new Agent({ system: "answer" });
  const g = new Graph({
    entry: "classify",
    nodes: { classify: triage, answer },
    edges: {
      classify: (m) => (m.content.includes("MATH") ? "answer" : "end"),
      answer: "end",
    },
  });
  const out = await g.trigger("What is 2 + 2?");
  expect(out.content).toBe("4");
});
