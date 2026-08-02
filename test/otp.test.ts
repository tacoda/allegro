import { expect, test, beforeEach } from "bun:test";
import {
  runtime,
  spawn,
  send,
  monitor,
  register,
  whereis,
  GenServer,
  Supervisor,
  child,
  Task,
} from "../src/otp/index.ts";

beforeEach(() => runtime.reset());

const tick = () => new Promise((r) => setTimeout(r, 10));

// --- bare actors + registry ---

test("actor echoes via the registry", async () => {
  const replies: string[] = [];
  const inbox = spawn((s: null, msg: any) => {
    replies.push(msg);
    return s;
  }, null);

  const echo = spawn((s: null, msg: any, ctx) => {
    ctx.send(msg.from, "echo: " + msg.text);
    return s;
  }, null);
  register("echo", echo);

  send(whereis("echo")!, { from: inbox, text: "hi" });
  await tick();
  expect(replies).toEqual(["echo: hi"]);
});

// --- GenServer ---

class Counter extends GenServer<number> {
  handleCast(_msg: string, state: number) {
    return state + 1;
  }
  handleCall(_msg: string, state: number) {
    return this.reply(state, state);
  }
}

test("genserver casts then calls", async () => {
  const c = await Counter.start(0);
  c.cast("inc");
  c.cast("inc");
  c.cast("inc");
  expect(await c.call("get")).toBe(3);
});

// --- crash isolation ---

test("a crashing actor does not take down the program", async () => {
  let downReason = "";
  const boom = spawn(() => {
    throw new Error("kaboom");
  }, null);
  monitor(boom, (reason) => (downReason = reason));
  send(boom, "go");
  await tick();
  expect(downReason).toBe("kaboom");
  // the runtime is still usable
  const c = await Counter.start(0);
  c.cast("inc");
  expect(await c.call("get")).toBe(1);
});

// --- supervision ---

class Worker extends GenServer<number> {
  handleCall(msg: string, state: number) {
    if (msg === "crash") throw new Error("worker down");
    return this.reply(state, state);
  }
}

test("supervisor restarts a crashed child with fresh state", async () => {
  const sup = await Supervisor.start({ children: [child(Worker, 42)] });
  const original = sup.whichChildren()[0]!;
  expect(await original.call("id")).toBe(42);

  await expect(original.call("crash")).rejects.toThrow();
  await tick();

  const restarted = sup.whichChildren()[0]!;
  expect(restarted.pid).not.toBe(original.pid);
  expect(await restarted.call("id")).toBe(42);
});

// --- Task ---

test("Task.parallel joins in input order", async () => {
  const out = await Task.parallel([() => 1 + 1, () => 2 * 3, async () => 10 - 4]);
  expect(out).toEqual([2, 6, 6]);
});
