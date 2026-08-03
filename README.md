# allegro

A Bun/TypeScript toolkit for building **agentic systems** as a **graph**. Declare
a system as a typed spec, then run it three ways — a headless CLI, a terminal UI,
or a web UI.

- **A system is a graph.** Every primitive is a **node** (`tool`, `fn`, `memory`,
  `skill`, `agent`, `mcp`, `graph`); nodes are wired by **transitions** (flow),
  **uses** (dependencies), and **triggers** (`commands` + `hooks`).
- **Deterministic where it should be.** `fn` nodes and router transitions are
  plain code — no LLM call unless a node is an `agent`.
- **The Claude primitive set.** agents, tools, skills, memory, commands, hooks,
  MCP servers — all as graph nodes and edges.
- **One typed spec, three surfaces** — `run` (headless), `tui` (Ink), `web`
  (`Bun.serve` + React), all fed by one lifecycle event stream.

```bash
bun install
bun run src/cli/main.ts run examples/01_hello.ts           # -> Hello, world!
bun run src/cli/main.ts run examples/09_triage.ts --mock   # agent example, canned backend
OPENAI_API_KEY=sk-... bun run src/cli/main.ts run examples/09_triage.ts
```

Every example runs two ways: `--mock` (a canned model backend, no key) or with a
real `OPENAI_API_KEY` in the environment.

## A system is a typed spec

```ts
import { defineSystem } from "allegro";

export default defineSystem({
  nodes: {
    notes:  { type: "memory" },
    shout:  { type: "tool", description: "uppercase text", run: (i) => i.toUpperCase() },
    triage: { type: "agent", system: "reply MATH or OTHER" },
    answer: { type: "agent", system: "be concise", uses: ["shout", "notes"] },
    size:   { type: "fn", run: (m) => String(m.content.length) },  // deterministic, no LLM
  },
  transitions: {
    entry:  "triage",
    triage: (m) => (m.content.includes("MATH") ? "answer" : "end"),
    answer: "end",
  },
  commands: { ask: { target: "triage", description: "Ask the desk." } },
  hooks: {
    preToolUse: { match: "shout", run: (e) => (e.input === "" ? { block: true } : undefined) },
  },
  run: async (sys) => {
    console.log((await sys.run("What is 2 + 2?")).content);
  },
});
```

Structure is data; behavior (tool/fn bodies, transition routers, hook handlers)
and `run` are inline TypeScript. Full schema in **[SPEC.md](SPEC.md)**.

## The graph model

| Kind | What | How |
|------|------|-----|
| **nodes** | vertices — the primitives | `tool` `fn` `memory` `skill` `agent` `mcp` `graph` |
| **transitions** | control flow (next node) | `entry` + name → name \| `"end"` \| `(msg) => next` |
| **uses** | dependencies (agent → tool/skill/agent/mcp/memory) | inline `uses: [...]` |
| **commands** | user-fired entrypoints | `{ target, description?, input? }` |
| **hooks** | event-fired interceptors (can block) | `{ match?, run(ev) }` per event |

An `agent` with a `description` that another agent `uses` becomes a **delegate**
(its own call, own context) — that's what a "subagent" was. A `skill`'s
instructions compose *into* an agent's prompt. A `graph` node nests recursively.

## Three surfaces

```bash
allegro run <spec.ts> [--events]                 # headless; --events streams the feed
allegro run <spec.ts> --command <name> [--input] # invoke a command
allegro tui <spec.ts>                            # terminal UI: nodes, events, output
allegro web <spec.ts> [--port n]                 # web UI at http://localhost:4173
```

All three consume the **same lifecycle event stream** (`agentStart`/`agentFinish`/
`preToolUse`/`postToolUse`/`nodeEnter`/`nodeExit`/`command`/`stop`/`log`) — the CLI
prints it, the TUI (Ink) and web (React) render it. It is also the substrate
hooks fire on.

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
Memory adds built-in `remember`/`recall` tools. Tool calls pass through
`preToolUse`/`postToolUse` hooks, which can block or replace them.

## CLI

```
allegro run <spec.ts|spec.json> [--events] [--command <name>] [--input <s>]
allegro tui <spec.ts>
allegro web <spec.ts> [--port <n>]
allegro help
```

## Build a single binary

```bash
bun run build        # -> ./allegro  (self-contained; run/tui/web all work)
./allegro run examples/01_hello.ts
```

## Examples

Ordered by complexity. `01–07` are deterministic (no LLM); `08–12` are agents,
runnable with `--mock` or a real key.

| # | file | shows |
|---|------|-------|
| 01 | `01_hello.ts` | one `fn` node |
| 02 | `02_branch.ts` | conditional router transition |
| 03 | `03_loop.ts` | a transition that loops back |
| 04 | `04_pipeline.ts` | chained `fn` stages |
| 05 | `05_commands.ts` | user-facing `commands` |
| 06 | `06_nested_graph.ts` | a `graph` node (recursive) |
| 07 | `07_lifecycle_hooks.ts` | `sessionStart`/`stop` hooks |
| 08 | `08_assistant.ts` | agent + tool + memory |
| 09 | `09_triage.ts` | routing between agents |
| 10 | `10_skill.ts` | skill composed into an agent |
| 11 | `11_mcp.ts` | mcp node expanded to tools |
| 12 | `12_capstone.ts` | router + tool + skill + memory + delegate + hook + command |

## Layout

```
src/runtime/  the event bus (observability + hook dispatch/gating)
src/agents/   agent, tool, memory, graph, mcp, message, openai
src/spec/     defineSystem + node types, build/run/load
src/ui/       shared view-model (event format + node table)
src/cli/      headless entry
src/tui/      Ink app
src/web/      Bun.serve + React client
examples/     typed .ts specs
test/         bun test
```

## Test

```bash
bun test          # agents (mocked OpenAI), spec, TUI, web
bun run typecheck # tsc --noEmit
```
