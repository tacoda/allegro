# allegro specs

A spec is a **typed TypeScript module** that declares a system and exports it via
`defineSystem`. Structure is data; behavior and the `run` entrypoint are inline
TypeScript. Run it with `allegro run <spec.ts>` (or `tui` / `web`).

```ts
import { defineSystem } from "allegro";
export default defineSystem({ /* ... */ });
```

A `.json` file is also accepted for the **behavior-free data subset** (no inline
functions); the typed `.ts` form is the primary one.

## Top-level keys

All optional. Primitives are keyed by name; the name becomes the accessor on the
`sys` handle passed to `run`.

| Key           | Type                                   | Notes |
|---------------|----------------------------------------|-------|
| `memory`      | `Record<string, Record<string,string>>` | name → seed entries (`{}` for empty) |
| `tools`       | `Record<string, ToolSpec>`             | `{ description, run }` |
| `subagents`   | `Record<string, SubagentSpec>`         | `{ description, model?, system?, tools? }` |
| `agents`      | `Record<string, AgentSpec>`            | `{ model?, system?, temperature?, tools?, subagents?, memory? }` |
| `servers`     | `Record<string, GenServerClass>`       | a `class X extends GenServer` |
| `graphs`      | `Record<string, GraphSpec>`            | `{ entry, nodes, edges }` |
| `supervisors` | `Record<string, SupervisorSpec>`       | `{ strategy?, maxRestarts?, children }` |
| `run`         | `(sys: System) => void \| Promise`     | drives the declared primitives |

References are by name: `agents.answer.tools: ["shout"]` points at
`tools.shout`; graph `nodes` name agents; supervisor `children` name servers.

## Inline behavior

| Where                     | Signature | Returns |
|---------------------------|-----------|---------|
| `tools.<t>.run`           | `(input: string)` | tool result (string) |
| GenServer `handleCast`    | `(msg, state)` | new state |
| GenServer `handleCall`    | `(msg, state)` | `this.reply(value, state)` |
| GenServer `init`          | `(...args)` | initial state (defaults to first arg) |
| graph edge router         | `(msg: Message)` | next node name, or `"end"` |
| `run`                     | `(sys)` | — |

## `sys` — the built system

`run(sys)` receives every declared primitive, instantiated and wired:

```ts
sys.tools.shout          // Tool
sys.memory.notes         // Memory
sys.agents.triage        // Agent
sys.subagents.translator // Subagent
sys.graphs.desk          // Graph
sys.supervisors.sup      // SupervisorRef  (.whichChildren())
sys.servers.Counter      // the class (sys.start(sys.servers.Counter, 0))
sys.start(Server, ...a)  // start a GenServer -> ServerRef
sys.runtime              // the process runtime (subscribe to events)
```

## Reference

```ts
import { defineSystem, GenServer } from "allegro";

class Counter extends GenServer<number> {
  init(n: number) { return n; }                       // optional
  handleCast(_msg: string, s: number) { return s + 1; }
  handleCall(_msg: string, s: number) { return this.reply(s, s); }
}

export default defineSystem({
  memory: { notes: {} },

  tools: {
    shout: { description: "Uppercase the text.", run: (input) => input.toUpperCase() },
  },

  subagents: {
    translator: { description: "translate to French", system: "Translate to French." },
  },

  agents: {
    triage: { system: "Reply MATH or OTHER." },
    answer: { system: "Be concise.", tools: ["shout"], memory: "notes", subagents: ["translator"] },
  },

  servers: { Counter },

  supervisors: {
    sup: { strategy: "one_for_one", maxRestarts: 5, children: [{ server: Counter, args: [0] }] },
  },

  graphs: {
    desk: {
      entry: "classify",
      nodes: { classify: "triage", answer: "answer" },
      edges: {
        classify: (msg) => (msg.content.includes("MATH") ? "answer" : "end"),
        answer: "end",
      },
    },
  },

  run: async (sys) => {
    console.log((await sys.graphs.desk!.trigger("What is 2 + 2?")).content);
  },
});
```

## Examples

- `examples/counter.ts` — a GenServer *(offline)*
- `examples/supervisor.ts` — crash isolation + restart *(offline)*
- `examples/assistant.ts` — an agent with a tool and memory *(needs `OPENAI_API_KEY`)*
- `examples/triage.ts` — a routing graph *(needs `OPENAI_API_KEY`)*
