# Allegro

**Allegro is a language to easily build agent harnesses.**

An Elixir-flavored functional language for composing AI agents into harnesses.
The agentic primitives (`Agent`, `Tool`, `Harness`) and an OTP-lite process
model (`Supervisor`, `Orchestrator`, `Factory`, `StateGraph`) compose into
full-scale systems — that composition is the point of the language, and the
part worth learning. It runs on a tree-walking interpreter in Rust, backed by
the OpenAI API.

```elixir
agent = Agent.new(system: "You are terse. One short sentence.")

{:ok, msg} = "What is the capital of France?" |> Agent.run(agent)
IO.puts(msg.content)          # => Paris.
```

Agents default their model from the `MODEL` env var (else `gpt-4o-mini`), the
provider from `PROVIDER` (else `openai`), and the key from `OPENAI_API_KEY`.

## Build & run

```bash
cargo build --release
./target/release/allegro run examples/comprehensions.al               # no network
OPENAI_API_KEY=sk-... ./target/release/allegro run examples/support_system.al
```

A program is mostly module definitions with a single invoke line at the bottom.
Files use the `.al` extension; `#` starts a comment.

---

## The language in a minute

If you know Elixir, you already know most of this. Same shapes: `defmodule` with
multi-clause, guarded, arity-aware `def`; `=` is pattern match; `|>` pipes;
`case`/`cond`/`with`/`if`; `for` comprehensions; `{:ok, v}` / `{:error, r}`
result tuples.

```elixir
defmodule Math do
  def add(a, b), do: a + b

  def factorial(0), do: 1
  def factorial(n) when n > 0, do: n * factorial(n - 1)
end

[1, 2, 3, 4]
|> Enum.map(fn x -> x * x end)
|> Enum.filter(fn x -> x > 4 end)     # [9, 16]

case Map.fetch(scores, :alice) do
  {:ok, value} -> value
  :error       -> :fallback
end

for x <- [1, 2, 3], x > 1, do: x * x  # [4, 9]
```

Values: integers/floats, atoms (`:ok`, and `true`/`false`/`nil`), strings with
`"#{interpolation}"`, lists `[h | t]`, tuples `{:ok, v}`, maps `%{a: 1}`
(accessed `m.a`), structs `%Point{x: 1}` (`defstruct [:x, :y]`), and functions
`fn x -> x end` (called `f.(x)`, captured `&Mod.fun/2`, shorthand `&(&1 + 1)`).

Standard library — data-first so everything pipes: `Enum` (map, filter, reject,
reduce, find, count, any?, all?, sum, join, sort, sort_by, member?), `String`,
`Map`, `List`, `Integer`, `Kernel` (auto-imported: `div`, `hd`, `elem`, `is_*`
guards, …), `IO` (puts, inspect).

**Extensions beyond Elixir-lite:** a trailing `*rest` variadic parameter
(`def log(level, *rest)`), and the process model below. Not (yet) present:
sigils, `alias`/`import`, map/struct update (`%{m | k: v}`), bracket access
(`m[:k]` — use `m.k` or `Map.get`).

---

## Core: building agent systems

This is the new part. Agentic primitives and OTP constructs are all **structs +
modules of functions**, so you compose them the same way you compose any data —
with pattern matching and the pipe.

### Agentic primitives

```elixir
weather = Tool.new(
  name: "get_weather",
  description: "Get the current weather for a city",
  run: fn city -> "sunny, 25C in #{city}" end
)

agent = Agent.new(
  model: Model.new(name: "gpt-4o-mini", temperature: 0.2),
  system: "Use the get_weather tool. Be terse.",
  tools: [weather]
)

{:ok, msg} = "Weather in Paris?" |> Agent.run(agent)   # model calls the tool, then answers
```

- **`Agent`** — `Agent.new(model:, system:, tools:)`. `Agent.run(input, agent)`
  returns `{:ok, %Message{}}` / `{:error, reason}` and drives the tool-calling
  loop; `Agent.run!/2` returns the bare message or raises; `Agent.fan_out(inputs,
  agent)` runs many inputs concurrently.
- **`Tool`** — `Tool.new(name:, description:, run:)`; the model invokes `run:`
  (given the tool's string input) mid-run via function calling.
- **`Model`** — `Model.new(provider:, name:, temperature:)`. Only `openai` is
  wired today; the struct exists so other providers slot in later.
- **`Message`** — an agent's output: `.content`, `.role`, `.from`.
- **`Harness`** — `%Harness{run: fn input -> {:ok, out} end}`; wraps a whole
  pipeline into one overridable, runnable unit.

### Orchestration & OTP-lite

The thing that *decides what runs* is an orchestrator or supervisor, not an
agent. Agents are leaf workers; sequencing, fan-out, routing, and restart are
ordinary Allegro modules over the process model.

- **`Orchestrator`** — `sequence(input, stages)` threads input through stages
  (short-circuits on `{:error, _}`); `parallel(input, stages)` fans each stage
  out to its own process and gathers results in order.
- **`Factory`** — a fixed pool of worker processes draining a job queue.
- **`Supervisor`** — runs child specs `{id, start}` as monitored processes,
  restarting on crash (`:one_for_one`, up to `max_restarts`).
- **`Retry`** — retry a fallible `{:ok,_}`/`{:error,_}` function with backoff.
- **`Loop`** — an agentic run-until-done loop.
- **`StateGraph`** — a graph of nodes over a shared state map (per-key reducers,
  conditional routing) with checkpointing and `resume` from any checkpoint.

### Putting it together — a support system

Agents as pipeline stages, wrapped in a `Harness`, made resilient with `Retry`,
metered with a `Store`, driven by a `for` comprehension over a batch. This is
`examples/support_system.al`, and it runs end to end against OpenAI:

```elixir
triage    = Agent.new(system: "Classify the ticket in one word: billing, tech, or other.")
responder = Agent.new(system: "You are a terse support agent. One sentence.")

# a two-stage pipeline wrapped as one runnable unit
desk = %Harness{run: fn ticket ->
  Orchestrator.sequence(ticket, [
    fn t ->
      case Agent.run(t, triage) do
        {:ok, m} -> {:ok, "[#{String.trim(m.content)}] #{t}"}
        err -> err
      end
    end,
    fn tagged ->
      case Retry.run(fn -> Agent.run(tagged, responder) end, 3) do   # resilient stage
        {:ok, m} -> {:ok, m.content}
        err -> err
      end
    end
  ])
end}

handled = Store.new(0)

for ticket <- ["My card was charged twice", "The app crashes on login"] do
  {:ok, answer} = Harness.run(ticket, desk)
  Store.update(handled, fn n -> n + 1 end)
  IO.puts("- #{answer}")
end
IO.puts("handled #{Store.get(handled)} tickets")
```

```
- Please check your transaction history and contact customer support for a refund.
- Try reinstalling the app or clearing its cache.
handled 2 tickets
```

**Other shapes that compose the same way:**

- **Supervised agent workers** — make each `Agent.run` a supervised child so
  transient network/rate-limit failures are restarted automatically
  (`Supervisor` + a `Store` counter that survives restarts →
  `examples/self_healing.al`).
- **Concurrent batch** — `Factory.run(tickets, fn t -> handle(t) end, pool_size)`
  drains a queue of work across a worker pool (`examples/worker_pool.al`).
- **Routing between agents** — a `StateGraph` whose nodes are agents and whose
  edges route on state, with every step checkpointed and resumable
  (`examples/state_graph.al`).

---

## Processes (the substrate)

The OTP constructs above are built on a cooperative, single-threaded,
run-to-completion **actor** scheduler. A process is `state + a handler`; `spawn`
starts one, `send` delivers a message, the handler runs to completion and
returns the new state. Handlers never block; only the top-level flow may
`receive`. Message isolation is free — values are immutable.

```elixir
defmodule Counter do
  def handle(n, {:inc, from}) do
    send(from, {:count, n + 1})
    {:noreply, n + 1}
  end
end

pid = spawn(Counter, 0)
send(pid, {:inc, self()})
receive do
  {:count, c} -> IO.puts("count is #{c}")
end
```

Primitives: `spawn/1,2`, `send/2`, `self/0`, `monitor/1` (delivers
`{:DOWN, pid, reason}` on death), `receive`/`after`. A **`Store`**
(`new`/`get`/`put`/`update`) is a mutable cell for state that must outlive a
call or survive a restart; the **registry** (`Process.register/2`,
`Process.whereis/1`, `send(:name, msg)`) binds names to pids.

Single-threaded by design: this provides the message-passing *programming model*
(mailboxes, queues, supervised workers), not true parallelism or multi-node
distribution.

---

## Examples

Every file under `examples/` is standalone and runnable.

| File | Shows |
|---|---|
| `support_system.al` | **full system** — agents + Orchestrator + Harness + Retry + Store |
| `agent.al` | Model / Tool / Agent / Harness against OpenAI |
| `worker_pool.al` | `Orchestrator.parallel` fan-out, `Factory` worker pool |
| `supervisor.al` | monitored children, restart on crash |
| `self_healing.al` | a flaky supervised child that recovers across restarts |
| `state_graph.al` | `StateGraph` — reducers, routing, checkpoints, resume |
| `state.al` | `Store` + the process registry |
| `processes.al` | spawn / send / receive, monitor, `after` |
| `comprehensions.al` | `for` — generators, filters, cartesian, map iteration |

## Status

Interpreted (tree-walking); no VM or compiler yet. Implemented: the functional
core, control flow, structs, the data standard library, `for` comprehensions,
the actor process model (`spawn`/`send`/`receive`/`monitor`, `Store`, registry),
OTP-lite (`Supervisor`/`Orchestrator`/`Factory`/`Retry`/`Loop`), `StateGraph`
with checkpointing, and the OpenAI-backed AI primitives.

See `PLAN.md` for the full design, locked decisions, and roadmap. Deferred:
sigils, `alias`/`import`, map/struct update, `Memory`, `for`
`into:`/`uniq:`/`reduce:`.
