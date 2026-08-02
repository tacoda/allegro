# allegro

A Bun/TypeScript toolkit for building **agentic systems** on an **OTP-style
process runtime**. Declare a system as a typed spec, then run it three ways — a
headless CLI, a terminal UI, or a web UI.

- **Agentic primitives** — Agent, Tool, Memory, Subagent, Graph, backed by the
  OpenAI API.
- **OTP process model** — GenServer, Supervisor, Registry, Task, spawn/send,
  monitor + restart-on-crash, on green threads (the JS event loop).
- **One typed spec, three surfaces** — `run` (headless), `tui` (Ink), `web`
  (`Bun.serve` + React), all fed by one runtime event stream.

```bash
bun install
bun run src/cli/main.ts run examples/counter.ts        # -> 3
OPENAI_API_KEY=sk-... bun run src/cli/main.ts run examples/triage.ts
```

## A system is a typed spec

```ts
import { defineSystem, GenServer } from "allegro";

class Counter extends GenServer<number> {
  handleCast(_msg: string, state: number) { return state + 1; }
  handleCall(_msg: string, state: number) { return this.reply(state, state); }
}

export default defineSystem({
  tools:  { shout: { description: "uppercase text", run: (input) => input.toUpperCase() } },
  agents: { triage: { system: "reply MATH or OTHER" },
            answer: { system: "be concise", tools: ["shout"] } },
  servers: { Counter },
  supervisors: { sup: { strategy: "one_for_one", children: [{ server: Counter, args: [0] }] } },
  graphs: {
    desk: {
      entry: "classify",
      nodes: { classify: "triage", answer: "answer" },
      edges: { classify: (m) => (m.content.includes("MATH") ? "answer" : "end"), answer: "end" },
    },
  },
  run: async (sys) => {
    const c = sys.supervisors.sup!.whichChildren()[0]!;
    c.cast("inc");
    console.log(await c.call("get"));               // 1
    console.log((await sys.graphs.desk!.trigger("What is 2 + 2?")).content);
  },
});
```

Structure is data; behavior (tool bodies, graph routers, GenServer callbacks) and
`run` are inline TypeScript. Full schema in **[SPEC.md](SPEC.md)**.

## Three surfaces

```bash
allegro run <spec.ts> [--events]   # headless; --events streams the runtime feed
allegro tui <spec.ts>              # terminal UI: process table, events, output
allegro web <spec.ts> [--port n]   # web UI at http://localhost:4173
```

All three consume the **same runtime event stream** (`spawn`/`exit`/`restart`/
`agent`/`log`) — the CLI prints it, the TUI (Ink) and web (React) render it.

## OTP process model

The JavaScript event loop **is** the cooperative green-thread scheduler:
`await` yields, `.call` awaits a reply promise, a crash is a caught `throw`. No
manual pumping.

```ts
import { GenServer, Supervisor, child, spawn, send, Task } from "allegro";

const c = await Counter.start(0);
c.cast("inc"); await c.call("get");                  // 1

const sup = await Supervisor.start({ children: [child(Counter, 0)] });
sup.whichChildren();                                 // restarted on crash

await Task.parallel([() => work(1), () => work(2)]); // green-thread fan-out
```

- **GenServer** — `init` / `handleCast` / `handleCall` (`reply(value, state)`);
  `.start` / `.call` / `.cast` / `.stop`.
- **Supervisor** — child specs + `one_for_one` restart with a budget.
- **Registry** — `register` / `whereis`; `send`/`monitor` take a pid or a name.
- **Task** — `async` / `await` / `parallel`.
- Bare actors — `spawn(handler, state)` + `send`; `monitor` delivers exits.

## Agentic primitives

```ts
import { Agent, Tool, Memory } from "allegro";

const bot = new Agent({
  system: "Use your tools.",
  tools: [new Tool({ name: "shout", description: "uppercase", run: (t) => t.toUpperCase() })],
  memory: new Memory(),
});
const msg = await bot.run("Please shout: hello");    // Message
await bot.fanOut(["a", "b"]);                         // concurrent, in order
```

Agents run a tool-calling loop over the OpenAI Chat Completions API. `model:`
defaults to the `MODEL` env var (else `gpt-4o-mini`); set `OPENAI_API_KEY`.
Memory adds built-in `remember`/`recall` tools. A **Graph** routes between
agents; a **Subagent** is a delegate.

## CLI

```
allegro run <spec.ts|spec.json> [--events]
allegro tui <spec.ts>
allegro web <spec.ts> [--port <n>]
allegro help
```

## Build a single binary

```bash
bun run build        # -> ./allegro  (self-contained; run/tui/web all work)
./allegro run examples/counter.ts
```

## Layout

```
src/otp/      process runtime: runtime, genserver, supervisor, registry, task
src/agents/   agent, tool, memory, subagent, graph, message, openai
src/spec/     defineSystem + types, build/run/load
src/ui/       shared view-model (event format + process table)
src/cli/      headless entry
src/tui/      Ink app
src/web/      Bun.serve + React client
examples/     typed .ts specs
test/         bun test
```

## Test

```bash
bun test          # OTP, agents (mocked OpenAI), spec, TUI, web
bun run typecheck # tsc --noEmit
```
