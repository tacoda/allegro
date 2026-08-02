# Allegro — Elixir-flavored functional language: rewrite plan

> Source files use the **`.al`** extension.

## 1. Goal & non-goals

**Tagline: Allegro is a language to easily build agent harnesses.**

**File convention.** A program/example is mostly *definitions* (`defmodule`s),
with a **single invoke line at the bottom** to run it standalone (e.g.
`Main.run()`).

**Goal.** Replace the Ruby-flavored object model with an **Elixir-flavored
functional** language for composing AI agents into harnesses. Same job as before (build
agents/tools/graphs backed by OpenAI, driven by env tokens), but the paradigm
is functional: data flows through pure-ish functions composed with the pipe
operator, primitives are **standard-library modules + structs**, and behavior is
selected by **pattern matching**.

**Non-goals (explicit).**
- **No BEAM / no VM / no bytecode.** Keep the current **tree-walking
  interpreter**. The binary reads a `.al` file and evaluates its AST directly.
- **No compilation yet.** Interpreted is the target for now; a compiler is a
  later, separate effort.
- **Process model — IN scope (D9, reverses the earlier non-goal).** An
  **actor scheduler**: cooperative, single-threaded, run-to-completion. A
  process is `state + a handler`; `spawn/send/self/monitor` + `receive`. No
  BEAM/VM, still no true parallelism or multi-node distribution — the
  *programming model* (message passing, mailboxes, queues, supervised workers)
  is what's provided, on one OS thread. `fan_out` stays a thread-pool helper.
- **OTP-lite is in scope**, now built on the actor scheduler: `Orchestrator`
  fans out to worker processes, `Factory` is a worker pool draining a queue,
  `Supervisor` monitors real processes and restarts them on crash. See §4.4.
- No macros/metaprogramming in phase 1 (`defmacro`), no protocols/behaviours in
  phase 1.

**Focus for this effort (from the brief):**
1. Enough Elixir feature parity to work with agents comfortably.
2. An easy, functional, composable way to build and combine AI primitives.

## 2. What we keep, replace, delete

| Component | Fate |
|---|---|
| `src/openai.rs` | **Keep as-is.** HTTP/Chat Completions layer is paradigm-agnostic. |
| `src/main.rs` | Keep shape (read file → lex → parse → eval); rewire to new interp. |
| `Cargo.toml` deps | Keep (`reqwest`, `serde_json`, threads). |
| `src/token.rs`, `src/lexer.rs` | **Rewrite** for Elixir tokens. |
| `src/ast.rs`, `src/parser.rs` | **Rewrite** for Elixir grammar. |
| `src/value.rs` | **Rewrite** value model (atoms, tuples, maps, structs, multi-clause functions). |
| `src/interp.rs` | **Rewrite** as functional evaluator + pattern matcher + module registry. |
| `src/builtins.rs` | **Replace** — Ruby constructors become std-lib module functions. Reuse the agent-assembly logic (`build_agent`) as the body of `Agent.new`. |
| `src/methods.rs` | **Delete** — no method dispatch; replaced by module functions (`Enum`, `String`, `Map`, …). |
| `examples/*.al`, `README.md` | **Rewrite** in the new syntax. |

The rewrite is a new front-end + evaluator that **reuses the OpenAI backend and
the agent-assembly logic**. It is not a from-scratch product.

## 3. Language design

### 3.1 Values

| Type | Literal | Notes |
|---|---|---|
| Integer | `42` | **Split from float** (Elixir-core). `div/2`, `rem/2` for ints; `/` returns float. |
| Float | `3.14` | |
| Atom | `:ok`, `:"with spaces"` | `true`, `false`, `nil` are atoms. |
| Boolean/nil | `true` `false` `nil` | Represented as atoms; truthiness: only `false`/`nil` are falsy. |
| String (binary) | `"hi #{name}"` | UTF-8; **interpolation** with `#{}`. |
| List | `[1, 2, 3]`, `[h \| t]` | Linked-list semantics; cons/`[h\|t]` pattern. |
| Tuple | `{:ok, value}` | Fixed-size; the idiom for tagged results. |
| Map | `%{:a => 1}`, `%{a: 1}` | `a: 1` sugar = `:a => 1`. Access `map[:a]`, `map.a` (struct/known key). |
| Keyword list | `[color: "teal", size: 3]` | Sugar for `[{:color, ...}, {:size, ...}]`; used for options. |
| Struct | `%Agent{model: "…"}` | Tagged map with a `__struct__: :Agent` key; defined by `defstruct`. |
| Function | `fn x -> x + 1 end` | Closure over env; **multiple clauses + guards**; arity-aware. |

**Decision D2 (flagged):** Integer/Float split vs a single `Number`. Recommend
**split** — it is core Elixir (`div`, `/`, `is_integer`) and cheap to add now;
retrofitting later is worse.

### 3.2 Operators

- Match: `=` (pattern match / bind, not assignment).
- Pipe: `x |> f(a, b)` ≡ `f(x, a, b)`.
- Arithmetic: `+ - * /`, `div`, `rem`.
- Comparison: `== != < > <= >=`, `===`.
- Boolean: `and or not` (strict, boolean-only) and `&& || !` (truthy).
- String concat: `<>`. List concat/diff: `++` / `--`.
- Capture: `&` / `&1` (phase 2).

### 3.3 Modules & functions

```elixir
defmodule Math do
  def add(a, b), do: a + b          # inline body

  def factorial(0), do: 1            # multiple clauses,
  def factorial(n) when n > 0 do     # pattern-matched heads + guards
    n * factorial(n - 1)
  end

  defp helper(x), do: x              # private
end

Math.add(2, 3)          # 5
2 |> Math.add(3)        # 5
```

- `def` / `defp`, `do ... end` or `, do:` short form.
- **Multi-clause** functions dispatched by pattern-matching the arguments,
  first matching clause wins; `when` guards restrict a clause.
- **Arity matters**: `foo/1` and `foo/2` are different functions.
- Dotted module names (`A.B.C`) for namespacing.
- Nested `defmodule` allowed.

### 3.4 Anonymous functions & capture

```elixir
double = fn x -> x * 2 end
double.(21)                 # 42  (Elixir's `.()` call syntax)

inc = &(&1 + 1)             # capture shorthand (phase 2)
run = &Agent.run/2          # function capture (phase 2)
```

### 3.5 Pattern matching & control flow

```elixir
{:ok, value} = {:ok, 42}          # match, binds value

case Agent.run(agent, input) do
  {:ok, %Message{content: c}} -> IO.puts(c)
  {:error, reason}            -> IO.puts("failed: #{reason}")
end

cond do
  x > 10 -> "big"
  x > 0  -> "small"
  true   -> "nonpos"
end

if ready?, do: go(), else: wait()

with {:ok, a} <- step1(),
     {:ok, b} <- step2(a) do
  {:ok, b}
end
```

Destructuring works for tuples, lists (`[h | t]`), maps (`%{key: v}`), and
structs (`%Agent{model: m}`). Pin operator `^` in phase 2.

### 3.6 Variadic functions (explicit request)

Elixir has no true variadic functions (it uses arity + lists). Since variadic
is explicitly wanted, add an **allegro extension**: a trailing rest parameter
collecting remaining args into a list.

```elixir
def log(level, *rest) do          # rest :: list
  IO.puts("#{level}: #{Enum.join(rest, " ")}")
end
log(:info, "a", "b", "c")         # rest = ["a", "b", "c"]
```

**Decision D5 (flagged):** accept the `*rest` extension, or stay
Elixir-pure (pass an explicit list)? Recommend the `*rest` extension — small,
and directly requested.

### 3.7 Comprehensions, sigils, interpolation

- String interpolation `"#{}"` — **phase 1** (needed everywhere).
- `for x <- list, filter, do: expr` — phase 5.
- Sigils (`~s`, `~w`) — later / optional.

## 3bis. Dynamic typing & dispatch by convention

**Dynamically typed, like Elixir.** Values carry their type at runtime; no
static checker, no declared signatures. (A static type system was considered
and set aside — see D6 in §8.)

Extensibility and "handle and delegate" come from **multi-clause functions that
pattern-match their arguments**, with a `_` **catch-all as the safeguard** —
already working since phase 1:

```elixir
def handle({:ok, value}), do: use(value)
def handle({:error, reason}), do: log(reason)
def handle(_other), do: :ignored          # catch-all safeguard
```

Wrapping a primitive is expressed the same way: match the shapes you specialize
and delegate the rest to the inner primitive through the catch-all — no type
system needed. **Variadic functions (`*rest`, D5)** pair with this: collect
varying arguments and dispatch on their shape.

## 3ter. Memory management

**Automatic, already.** Values are `Rc`-counted and freed when the last
reference drops — users never manage memory. The immutable functional value
model does not form reference cycles in normal use, so refcounting suffices; a
cycle collector is a later concern only if mutable cyclic structures appear.

## 4. Standard library & namespaces

### 4.1 Module system

- **Global module registry**: fully-qualified name → functions keyed by
  `{name, arity}`.
- `Kernel` is **auto-imported** (unqualified builtins: `is_nil/1`, `to_string/1`,
  `elem/2`, `hd/1`, `tl/1`, `length/1`, `map_size/1`, arithmetic guards, etc.).
- `alias Allegro.Agent` → refer to it as `Agent`.
- `import Enum` → call `map/2` unqualified (phase 3; used sparingly).
- `require` is a no-op stub (no macros yet).

### 4.2 Data std lib (phase 3)

`Enum` (map, filter, reduce, each, count, find, any?, all?, sort_by, join,
into, with_index), `String` (upcase, downcase, trim, split, contains?, length,
replace), `Map` (get, put, keys, values, merge, has_key?), `List`
(first/last/flatten/…), `Integer`/`Float`, `IO` (puts, inspect), `Kernel`.
Data-first arguments everywhere so everything pipes.

### 4.3 AI primitives as std lib (phase 4)

Each former primitive becomes a **struct + a module of functions**. This is the
"easy, functional composition" goal: primitives are plain data, transformed by
module functions and composed with `|>`.

```elixir
# build (structs), then run (transform data through them)
agent =
  Agent.new(
    system: "You are a concise front desk.",
    tools: [Tool.new(name: "room_for", run: fn dept -> lookup(dept) end)],
    memory: Memory.new()
  )

{:ok, msg} = "Which room is billing?" |> Agent.run(agent)
IO.puts(msg.content)
```

Two tiers — high-level concepts take customizations; low-level pieces are the
building blocks they compose from. Both are structs + modules (`new/1` +
operations), so either tier can be defined or overridden by the user.

- **Low-level (building blocks):** `Model`, `Tool`, `Context` (assembled model
  input: system + history + memory + retrieved docs + tools), `Memory`, `Rule`,
  `Skill`, `Hook`, `Command`, `Message`.
- **High-level (take config/customization):** `Agent`, `Charter` (governance
  bundle), `Harness` (composes into one runnable unit), `Graph` (routing),
  `Factory` (worker queue), `Loop` (run-until-done / agentic loop),
  `Orchestrator`, `Supervisor`, `Subagent`.

`Agent.new/1` reuses the old `build_agent` assembly logic. High-level concepts
are mostly written in allegro over the low-level pieces + prelude; only `Model`/
`Agent.run`/`Tool` execution and mutable stores need Rust.

### 4.4 Supervision (OTP-lite, no processes)

Supervisors + **self-healing** without BEAM processes: cooperative and built on
the error channel (a `raise`/crash carries a value; the supervisor catches it).

- A **child spec** is a callable (an `fn`, an agent runner, a `{mod, fun, args}`
  triple) plus a `:restart` policy (`:permanent | :transient | :temporary`).
- `Supervisor.start(children, strategy: :one_for_one, max_restarts: 3)` runs the
  children; when one crashes, the supervisor restarts it per the strategy
  (`:one_for_one` restarts just that child; `:one_for_all` restarts all) until
  `max_restarts` is exceeded, then it gives up and surfaces `{:error, _}`.
- Self-healing agent example: wrap a flaky `Agent.run` as a supervised child so
  transient failures (network, rate limits) are retried automatically.
- Scope: single-threaded and synchronous — supervision = structured retry/
  restart with strategies, not live concurrent processes. `GenServer`/mailboxes
  are out of scope.

### 4.5 Orchestration (agents are workers, not orchestrators)

Composition is driven by **orchestration/supervision constructs, not by an
agent delegating to sub-agents.** An `Agent` is a leaf worker: given input, it
produces output. Sequencing, routing, fan-out, and retry are the job of:

- `Graph` — control-flow routing between agent nodes (already exists).
- `Orchestrator` — composes agents into a pipeline/plan and runs them
  (sequential, parallel/`fan_out`, conditional) — the top-level driver.
- `Supervisor` — runs composed agents with restart strategies (§4.4).

This replaces the old agent-as-orchestrator model (a top-level agent holding
`subagents` and calling `delegate`). Sub-agents may remain as *named workers*,
but the thing that *decides what runs* is an orchestrator/supervisor, not an
agent. Keeps composition functional and inspectable rather than hidden inside a
model's tool-calls.

**Decision D1 (flagged) — pipeline subject convention.** In a functional agent
pipeline the thing that flows is the **message/data**, so recommend
**data-first**: `Agent.run(input, agent)`, `Tool.apply(input, tool)`,
`Graph.trigger(input, graph)`. Reads as:

```elixir
input
|> Agent.run(agent)
|> Message.content()
|> Tool.apply(shout)
```

Builders (`Agent.new/1`) return the struct and are not part of the data
pipeline. (Alternative: agent-first `agent |> Agent.run(input)`. Pick one and
apply consistently — it defines the whole std-lib API shape.)

**Decision D3 (flagged) — namespace.** Top-level (`Agent`, `Tool`) for
ergonomics, vs prefixed (`Allegro.Agent`, `AI.Agent`) for future namespaces
(REST etc.). Recommend **canonical prefixed modules (`Allegro.Agent`) with a
default `alias`** so users write `Agent` but new namespaces
(`Allegro.REST`, `Allegro.HTTP`) slot in cleanly later.

**Decision D4 (flagged) — result convention.** `{:ok, value}` /
`{:error, reason}` tuples (idiomatic, matches with `case`/`with`) vs bare
return + raise. Recommend **result tuples** for fallible ops (`Agent.run`,
tool/network), with `!` bang variants (`Agent.run!/2`) returning bare value or
raising. This is what makes `case`/`with` pull their weight.

### 4.4 Env-defaulted, inline config

Options are keyword lists merged over environment-derived defaults:

```elixir
Agent.new(system: "…")     # model  <- MODEL env  (else "gpt-4o-mini")
                           # provider <- PROVIDER env (else "openai")
                           # api key  <- OPENAI_API_KEY env
```

A small `Config` module centralizes env lookup: `Config.fetch(:model, default)`.

## 5. Interpreter architecture (tree-walking, functional)

- **Values** (`value.rs`): new enum per §3.1. Structs = map + struct-name tag.
  Functions carry a `Vec<Clause>` (params patterns, optional guard, body,
  captured env) + arity.
- **Pattern matcher** (`match.rs`, new): `match(pattern, value, &mut Bindings)
  -> bool`. Handles literals, vars (bind), `_`, tuples, lists/cons, maps,
  structs, pin (phase 2).
- **Module registry**: `HashMap<String, Module>`; `Module` = `HashMap<(String,
  usize), Rc<Function>>`. Populated by evaluating `defmodule`/`def` at load.
  Std-lib modules (Kernel/Enum/…/Agent/…) registered natively as Rust fns.
- **Native functions**: a `NativeFn` variant so std-lib is Rust but callable
  and pipeable like user functions.
- **Evaluator**: expression-oriented (everything returns a value). `case`,
  `cond`, `if`, `with`, blocks all yield values. Bindings are immutable within
  a scope; matches introduce new bindings (rebinding allowed, no mutation).
- **Function call**: resolve `{name, arity}` (or captured fn) → select first
  clause whose param patterns match (and guard passes) → eval body in a fresh
  scope with the bindings. No-clause-matches → `FunctionClauseError`.
- **Guards**: evaluate a restricted expr subset; failure = clause skipped.
- **Pipe**: desugared in the parser (`a |> f(b)` → `f(a, b)`), so the evaluator
  never sees `|>`.
- **Errors**: a `raise`/exception channel carrying a value; `{:error, _}` is
  ordinary data (not an exception). Bang functions raise.

## 6. Rewrite strategy

- Work on branch `feat/elixir-syntax` (already created).
- Land **phase by phase**, each phase building + running a verification `.al`
  script (no network for phases 1–3; phase 4 hits OpenAI).
- Keep `openai.rs` untouched; port `build_agent` logic into `Agent.new`.
- Rewrite `examples/` last (phase 5) so they track the final API; keep a couple
  of tiny throwaway scripts per phase for verification.
- Old Ruby-era code is replaced wholesale, not incrementally bridged (cleaner
  than maintaining two paradigms in one tree).

## 7. Phases & acceptance criteria

**Phase 1 — Core functional core. ✅ DONE.**
Values (int/float/atom/bool/nil/string+`#{}`/list/tuple/map/keyword),
`defmodule` + multi-clause `def` + guards + arity, `=` match with
tuple/list/`[h|t]`/map destructure, qualified + local calls, `Kernel`/`IO`,
arithmetic (`/` vs div/rem), comparison, `<>`/`++`/`--`, boolean, `if`, pipe.
*Verified:* the phase-1 script runs; multi-clause/guards/`if` work.

**Phase 2 — Control flow & anonymous functions. ✅ DONE.**
`case`, `cond`, `with` (+else), `unless`, `fn` + `.()` call, `&`/`&1` and
`&Mod.fun/n` capture, pin `^`, `IO.debug`/`inspect`/`write`.
*Verified:* `case` on `{:ok,_}/{:error,_}`, `with` chain, `fn` passed and called.

**Phase 3 — Structs & data std lib. ✅ DONE.**
`defstruct` (defaults, literal/pattern/update, as-patterns, `*rest` variadic);
`Enum` (map/filter/reject/each/reduce/find/count/any?/all?/sum/join/sort/
sort_by/member?), `String`, `Map` (fetch→`{:ok,_}`/`:error`), `List`, `Integer`.
*Verified:* `[1,2,3,4] |> Enum.map(fn x -> x*x end) |> Enum.sum()`; struct round-
trip; `*rest`. (`alias`/`import` deferred to phase 4 where `Allegro.*` aliasing
matters.)

**Phase 4 — AI primitives, supervision & orchestration.**
Key approach: an **embedded stdlib prelude** (allegro `.al` compiled into the
binary via `include_str!`, registered before user code). Most OTP/graph/agent
patterns are ordinary allegro modules over structs/results/recursion — only what
touches the network or mutable state needs Rust. Self-healing runs on
`{:ok,_}`/`{:error,_}` **data** matched with `case`, not exceptions.

- **4a (no network):** prelude mechanism; `Supervisor` (strategies
  `one_for_one`/`one_for_all`/`rest_for_one`, `max_restarts`); `Retry`
  (exponential backoff + jitter); `Orchestrator` (sequential/parallel/
  conditional composition of children). All allegro stdlib; verified with mock
  `{:ok}`/`{:error}` children. Native `Process.sleep/1` for backoff.
- **4b (network):** ✅ verified against OpenAI. `Agent`/`Tool`/`Model`/`Message`
  on `openai.rs` (native, `src/prims.rs`), env-default config (MODEL/PROVIDER/
  OPENAI_API_KEY), `{:ok,_}`/`{:error,_}` + `run!` bang, the tool-calling loop,
  and `fan_out` (concurrent). The high-level **`Harness`** (struct + module,
  overridable) composes an agent/tool pipeline into one runnable unit. See
  `examples/agent.al` (concierge answers via a `room_for` tool call; a Harness
  wraps the agent). Deferred: `Memory`, `alias`/`import` sugar.
- **4c:** ✅ done. `Store` (native mutable cell `Value::Ref` for cross-call
  state) and `Registry` (`Process.register/2`, `Process.whereis/1`, `send` by
  name) — see `examples/state.al`, `examples/self_healing.al` (Store persists
  across supervisor restarts, so a flaky child recovers). `GenServer`-lite is
  already covered by actors (`handle/2` + state). `StateGraph` (nodes over a
  shared state map, per-key reducers, conditional routing) + checkpointing +
  `resume` from any checkpoint — allegro stdlib over `Store`/`Map`; see
  `examples/state_graph.al`.
- **4d:** `Planner` (agent writes+executes a `Plan`); optional `raise`/`rescue`.
*Verify:* a flaky supervised child recovers; a StateGraph accumulates state; an
agent pipeline runs against OpenAI.

**Phase 5 — Ergonomics, docs & tutorial.**
`for` comprehensions, sigils (optional), `README.md`, and **a 25-file tutorial**
in `examples/` of growing complexity — each a standalone, runnable `.al` (01
basics → 25 full multi-agent composition).
*Verify:* every one of the 25 examples runs.

- ✅ `for` comprehensions: generators (`pat <- enum`), filters, multiple
  (cartesian) generators, map iteration (`{k, v} <-`), pattern-filtering
  (non-matching generator element is dropped), `do:`/`do…end`; yields a list,
  bindings scoped to the comprehension. See `examples/comprehensions.al`.
  Deferred: `into:`, `uniq:`, `reduce:`.
- ✅ `README.md` — language kept light (Elixir-flavored); the core is agentic +
  OTP composition, centered on a runnable full-scale example
  (`examples/support_system.al`: agents as Orchestrator stages, wrapped in a
  Harness, made resilient with Retry, metered with a Store, driven by `for`).
- Remaining: sigils (optional), and the rest of the tutorial set.

**Deferred (undecided):** static types (D6). Decoration (D8) cancelled.

## 8. Decisions (LOCKED)

- **D1 Pipeline subject — DATA-FIRST.** The message/prompt flows through `|>`;
  the agent/struct is a later argument. `Agent.run(input, agent)`,
  `Tool.apply(input, tool)`, `Graph.trigger(input, graph)`. Builders
  (`Agent.new/1`) return structs and sit outside the data pipeline.
- **D2 Numbers — INTEGER/FLOAT SPLIT.** `42` int, `3.14` float; `/` yields
  float, `div/2`/`rem/2` for ints, `is_integer/1`/`is_float/1` guards.
- **D3 Namespace — PREFIXED + DEFAULT ALIAS.** Canonical `Allegro.Agent`,
  implicitly aliased to `Agent`. Future namespaces (`Allegro.REST`,
  `Allegro.HTTP`) added the same way.
- **D4 Results — TUPLES + BANG VARIANTS.** Fallible ops return `{:ok, value}` /
  `{:error, reason}`; `!` variants (`Agent.run!/2`) return the bare value or
  raise. Drives `case`/`with`.
- **D5 Variadic — `*rest` EXTENSION.** Trailing rest parameter collects
  remaining args into a list. Central to dispatch-by-convention.
- **D6 Types — DYNAMIC (static types dropped).** Dynamically typed like Elixir;
  no checker, no declared signatures. Extensibility via multi-clause pattern
  matching + `_` catch-all, not a type system.
- **D7 nil KEPT.** Elixir-consistent. Option = `t | nil`; Result =
  `{:ok, t} | {:error, e}` (Rust semantics, Elixir clothing).
- **D8 Decoration — CANCELLED.** No protocol/behaviour, no decoration feature.
  Wrapping/delegation, when needed, is just ordinary multi-clause dispatch.
- **D9 Process model — ACTOR/STEP SCHEDULER (reverses the earlier "no
  processes" non-goal).** Cooperative, single-threaded, run-to-completion:
  a process is `state + handler`, driven by `spawn/2`, `send/2`, `self/0`,
  `monitor/1`, and `receive`. Handlers never block (they return the new
  state); only the root flow may `receive` (it sits at the base of the Rust
  stack, so blocking there is just running the driver loop inline). Rejected:
  stackful coroutines (new dep + `unsafe` reentrancy) and a stackless evaluator
  rewrite — the actor model needs neither and keeps the tree-walker untouched.
  Message isolation is free because values are immutable `Rc`. `Orchestrator`/
  `Factory`/`Supervisor` are ordinary allegro stdlib over these primitives.

## 9. Risks

- **Scope**: this is a language rewrite; phases 1–3 are the bulk of the work and
  have no AI payoff until phase 4. Mitigation: strict phase gating with runnable
  verification each step.
- **Pattern matcher correctness**: the core of the whole language. Mitigation:
  a dedicated `match.rs` with focused unit tests before it is wired into calls.
- **API churn**: D1/D3 shape every std-lib signature. Lock them before phase 4.
