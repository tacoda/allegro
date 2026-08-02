# Allegro → Elixir-flavored functional language: rewrite plan

## 1. Goal & non-goals

**Goal.** Replace the Ruby-flavored object model with an **Elixir-flavored
functional** language for composing AI agents. Same job as before (build
agents/tools/graphs backed by OpenAI, driven by env tokens), but the paradigm
is functional: data flows through pure-ish functions composed with the pipe
operator, primitives are **standard-library modules + structs**, and behavior is
selected by **pattern matching**.

**Non-goals (explicit).**
- **No BEAM / no VM / no bytecode.** Keep the current **tree-walking
  interpreter**. The binary reads a `.al` file and evaluates its AST directly.
- **No compilation yet.** Interpreted is the target for now; a compiler is a
  later, separate effort.
- No processes/actors/OTP, no concurrency model, no distribution. (`fan_out`
  stays a thread-pool helper as today, not a process model.)
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

## 3bis. Type system (pillar)

Rust-influenced **static typing in Elixir clothing**: types are inferred
everywhere, **every definition declares its signature**, and union types are
first-class. Kept minimal — only what the foundation and agent/network work
need. Runs as a checker pass after parse, before eval (untyped eval unchanged).

### Types

```
integer | float | number | boolean | atom | string | nil
list(t) | {t1, t2, ...} (tuple) | map | %{key: t} | fun(a, b -> c)
t1 | t2            # union
:ok | :error       # atom literals as singleton types
{:ok, t} | {:error, e}   # Result
t | nil                  # Option (nil kept, Elixir-consistent)
dynamic            # escape hatch for interop the checker can't see (network)
a, b, ...          # lowercase = type variables (parametric signatures)
```

### Declared definitions (mandatory)

Signatures are required on every `def`/`defp` using Elixir's `::` operator,
inline on params and return:

```elixir
def add(a :: integer, b :: integer) :: integer do
  a + b
end

def fetch(m :: map, k :: atom) :: {:ok, dynamic} | {:error, atom} do
  ...
end
```

- **Params and return type are required** (this is the "definitions always
  declare them" rule). The checker verifies the body's inferred type is
  assignable to the declared return, and that call sites pass assignable args.
- **Everything else is inferred**: locals, literals, `case`/`if` results,
  anonymous functions.
- **Union assignability**: `t` → `t1 | t2` if `t` fits either; `t1 | t2` → `u`
  if both fit. `dynamic` is assignable both ways (gradual boundary).
- **Type variables**: lowercase names in a signature unify, giving parametric
  functions (`Enum.map(list(a), fun(a -> b)) :: list(b)`) without full generics.

### User types & decorating primitives (priority)

The headline use case: **decorate core agentic primitives** — wrap an `Agent`
(or `Tool`, `Graph`, …) to add or override behavior while it still works
everywhere the primitive works. The Elixir idiom, and ours:

- **`defstruct`** defines a user type that embeds a primitive plus extra state.
- The type's **module** adds/overrides functions, delegating to the inner
  primitive for the rest.
- A minimal **protocol** (`defprotocol`/`defimpl`) — the agentic interface,
  e.g. `Runnable.run(x, input)` — lets a decorated struct satisfy the same
  interface and be dropped into any slot that expects the primitive (graph
  node, factory worker, pipeline stage). This is the polymorphism decoration
  needs; kept to the single interface the primitives share.

```elixir
defmodule Logged do
  defstruct [:inner :: dynamic]
  # decorate: same interface, extra behavior, delegates the rest
  def run(%Logged{inner: a} :: Logged, input :: string) :: {:ok, dynamic} | {:error, atom} do
    IO.puts("→ #{input}")
    Allegro.Agent.run(input, a)
  end
end

logged = %Logged{inner: Allegro.Agent.new(system: "…")}
"hello" |> Logged.run(logged)     # works anywhere an agent runner is expected
```

### Scope discipline

Monomorphic + gradual: no full Hindley-Milner, no typeclasses, no recursive
user type aliases beyond a simple `@type` (optional, late). **Protocols are
limited to the shared agentic interface**, not a general typeclass system.
`dynamic` covers what inference can't reach so the checker never blocks work.

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

Modules (each with `new/1` + operations): `Model`, `Agent`, `Subagent`, `Tool`,
`Memory`, `Rule`, `Skill`, `Hook`, `Command`, `Charter`, `Harness`, `Graph`,
`Factory`. `Agent.new/1` reuses today's `build_agent` assembly logic.

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

**Phase 2 — Control flow & anonymous functions.**
`case`, `cond`, `with`, `unless`, `fn` + `.()` call, `&`/`&1` capture, pin `^`.
(Multi-clause defs & guards already landed in phase 1.)
*Verify:* `case` on `{:ok,_}/{:error,_}`, a `with` chain, `fn` passed and called.

**Phase 3 — Type system (pillar).**
`::` signatures **required** on every `def`/`defp`; a `Type` model with base
types, `list(t)`, tuple/map/fun types, **unions**, atom-literal singletons,
type variables, and `dynamic`; a checker pass (infer bodies, verify declared
returns, check call args, union assignability). Result/Option types.
*Verify:* a typed module type-checks; an argument/return mismatch is rejected
with a clear error; a parametric signature unifies.

**Phase 4 — Structs, protocols & data std lib.**
`defstruct` (typed user types), the `Runnable` **protocol** for decoration
(`defprotocol`/`defimpl`, dispatch on struct tag); `Enum`, `String`, `Map`,
`List`, `Integer`, `IO`; `alias`/`import`. Typed signatures throughout.
*Verify:* `[1,2,3] |> Enum.map(fn x -> x*x end) |> Enum.sum()`; a decorated
struct satisfies `Runnable` and is used where the base type is expected.

**Phase 5 — AI primitives as std lib.**
Structs + modules for all primitives (`Allegro.*`, default-aliased), wired to
`openai.rs`; env-default inline config; `{:ok,_}`/`{:error,_}` + bang variants;
pipe composition; tool loop; memory; graph routing; delegation; fan_out;
**decoration** of `Agent`/`Tool`/`Graph` via structs + the `Runnable` protocol.
*Verify:* rebuilt equivalents of today's examples run against OpenAI; a
decorated agent runs in a graph node.

**Phase 6 — Ergonomics, docs & tutorial.**
`*rest` variadic, `for` comprehensions, sigils (optional), `@type` aliases
(optional), `README.md`, and **a 25-file tutorial** in `examples/` of growing
complexity — each a standalone, runnable `.al` (01 basics → 25 full multi-agent
composition with decoration).
*Verify:* every one of the 25 examples runs.

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
  remaining args into a list (allegro extension over Elixir).
- **D6 Types — STATIC, DECLARED-ON-DEFS, INFERRED ELSEWHERE.** `::` signatures
  required on every def; union types; type variables; `dynamic` gradual escape.
  Monomorphic + gradual, no full HM. Checker pass before eval.
- **D7 nil KEPT.** Elixir-consistent. Option = `t | nil`; Result =
  `{:ok, t} | {:error, e}` (Rust semantics, Elixir clothing).
- **D8 Decoration — STRUCTS + ONE PROTOCOL.** User types via `defstruct` +
  module; a single `Runnable` protocol lets decorated primitives satisfy the
  shared agentic interface. No general typeclass system.

## 9. Risks

- **Scope**: this is a language rewrite; phases 1–3 are the bulk of the work and
  have no AI payoff until phase 4. Mitigation: strict phase gating with runnable
  verification each step.
- **Pattern matcher correctness**: the core of the whole language. Mitigation:
  a dedicated `match.rs` with focused unit tests before it is wired into calls.
- **API churn**: D1/D3 shape every std-lib signature. Lock them before phase 4.
