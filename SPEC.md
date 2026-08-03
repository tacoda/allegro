# allegro specs

**A system is a graph.** Every primitive is a **node**; nodes are wired by three
kinds of edge:

- **transitions** — control flow (which node runs next). Routers are plain code, so conditionals live here.
- **uses** — dependencies (an agent's tools, skills, delegates, memory).
- **triggers** — `commands` (user-fired) and `hooks` (event-fired).

A spec is a typed TypeScript module exporting `defineSystem`. Structure is data;
behavior (`run` bodies, transition routers, hook handlers, the `run` entrypoint)
is inline TypeScript. Run it with `allegro run <spec.ts>` (or `tui` / `web`). A
`.json` file is accepted for the behavior-free data subset.

```ts
import { defineSystem } from "allegro";
export default defineSystem({ nodes: { /* … */ }, transitions: { /* … */ } });
```

## Node types

Each entry under `nodes` has a `type`. References between nodes are by name.

| `type` | shape | role | LLM |
|--------|-------|------|-----|
| `tool` | `{ description, run }` | code capability, model-invoked | — |
| `fn` | `{ run(msg) }` | deterministic flow step / control flow | no |
| `memory` | `{ seed? }` | key/value store (adds remember/recall) | no |
| `skill` | `{ description, instructions, uses? }` | instructions composed *into* an agent | no |
| `agent` | `{ description?, system?, model?, temperature?, uses? }` | LLM actor; delegatable if it has `description` | yes |
| `mcp` | `{ server, env?, tools? }` | external MCP server → expands to tools | — |
| `graph` | `{ nodes, transitions }` | composite (recursive) | — |

**Flow vs resource.** `agent`, `fn`, and `graph` are *flow* nodes — transitions
route among them. `tool`, `skill`, `memory`, `mcp` are *resources* — referenced
by an agent's `uses`, never routed to.

## `uses` — dependency edges

One list per agent (and skill). Each name resolves by the target's type:

- `tool` / `mcp` → a callable the model may invoke
- `skill` → its `instructions` are prepended to the agent's system prompt, its tools folded in
- `agent` → exposed as a delegation tool (its own call, own context) — this is what a "subagent" was
- `memory` → attached as the agent's store

```ts
answer: { type: "agent", system: "Be concise.", uses: ["shout", "notes", "translator"] }
```

## `transitions` — flow edges

`entry` names the first node; every other key maps a node to its next. A value is
a target name, `"end"`, or a router `(msg) => nextName`. Routers are deterministic
code — conditionals, switches, loops (route back to an earlier node).

```ts
transitions: {
  entry: "triage",
  triage: (msg) => (msg.content.includes("MATH") ? "answer" : "end"),
  answer: "end",
}
```

`"entry"` is reserved — don't name a node `entry`.

## `commands` — user entrypoints

Named ways to enter the graph from outside. `allegro run <spec> --command <name> [--input <s>]`, or `sys.command(name, input)`.

```ts
commands: { review: { target: "reviewer", description: "Review a PR", input?: "…" } }
```

## `hooks` — event triggers

Fire on lifecycle events; a `preToolUse` hook may **block** or **replace** a tool
call. `match` is a substring filter on the tool/agent name.

```ts
hooks: {
  preToolUse: { match: "shell", run: (e) => e.input?.includes("rm -rf") ? { block: true, reason: "no" } : undefined },
  postToolUse: { run: (e) => void log(e.tool) },
}
```

Events: `sessionStart`, `userPromptSubmit`, `preToolUse`, `postToolUse`,
`agentStart`, `agentFinish`, `stop`. Only `preToolUse` acts on the return value.

## `sys` — the built system

`run(sys)` receives the instantiated system:

```ts
sys.run(input)          // trigger the root graph -> Message
sys.command(name, in?)  // enter via a command -> Message
sys.graph               // the root Graph
sys.agents.answer       // Agent, by name
sys.tools.shout         // Tool
sys.memory.notes        // Memory
sys.graphs.pipeline     // a nested graph node
sys.nodes               // every instantiated node by name
```

## Reference

```ts
import { defineSystem } from "allegro";

export default defineSystem({
  nodes: {
    notes:      { type: "memory" },
    shout:      { type: "tool", description: "Uppercase.", run: (i) => i.toUpperCase() },
    translator: { type: "skill", description: "to French", instructions: "Translate replies to French." },

    triage:     { type: "agent", system: "Reply MATH or OTHER." },
    answer:     { type: "agent", system: "Be concise.", uses: ["shout", "notes", "translator"] },

    parse:      { type: "fn", run: (m) => String(m.content.length) },
  },

  transitions: {
    entry:  "triage",
    triage: (msg) => (msg.content.includes("MATH") ? "answer" : "end"),
    answer: "end",
  },

  commands: { ask: { target: "triage", description: "Ask the desk." } },

  hooks: { stop: { run: () => void console.log("done") } },

  run: async (sys) => {
    console.log((await sys.run("What is 2 + 2?")).content);
  },
});
```

## Examples

- `examples/pipeline.ts` — deterministic control flow: `fn` nodes, a router, a hook, a command *(offline)*
- `examples/assistant.ts` — an agent with a tool and memory *(needs `OPENAI_API_KEY`)*
- `examples/triage.ts` — a routing graph *(needs `OPENAI_API_KEY`)*
