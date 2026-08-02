# allegro

A small Ruby-like language for composing **agents, harnesses, graphs, and
runner queues** on an **OTP-style process runtime** (GenServers, supervisors,
green-threaded message passing). Primitives are classes: build with `.new`,
subclass for reuse, mix in modules for shared behavior, then invoke.

```ruby
bot = Agent.new(system: "You are terse.")   # model defaults from the MODEL env var

puts bot.run("Capital of France?").content   # => Paris.
```

Agents are backed by the OpenAI Chat Completions API. Set `OPENAI_API_KEY` in your
environment. `model:` defaults to the `MODEL` env var (else `gpt-4o-mini`) and
`provider:` to the `PROVIDER` env var (else `openai`), so an agent needs no
explicit config to run.

## Run

```bash
cargo build --release
OPENAI_API_KEY=sk-... ./target/release/allegro run examples/workflow.al
```

## Language

Ruby-flavored: paren-less calls, `def ... end`, `if/elsif/else/end`, `while`, `for … in`,
`#` comments, blocks close with `end`. Values: numbers, strings, `true`/`false`, `nil`,
arrays `[…]`, hashes `{ key: value }`.

```ruby
x = 3
if x < 5
  puts "small"
end

for n in range(1, 4)
  puts n            # 1 2 3
end

def greet(name)
  return "hi, " + name
end
```

### Pattern matching

`match` dispatches on a value by type, literal, or binding:

```ruby
match reply
when "yes"        # literal
  puts "affirmative"
when Number       # type: Nil Bool Number String Array Hash Agent Message ...
  puts "a number"
when other        # a bare name binds the value
  puts "got: " + str(other)
end
```

### Anonymous functions

`def (params) ... end` is a value — used for hooks, commands, tools, edges:

```ruby
double = def (x) return x * 2 end
puts double(21)   # 42
```

## Primitives

Primitives are **classes**. Build one with `.new` and Ruby-style keyword args,
keep the result, and invoke it later with a method. `.new` takes a config
(`Agent.new(model: "…", system: "…")`); omit it for defaults (`Memory.new`).

| Primitive  | Constructor | Invoke with |
|------------|-------------|-------------|
| `Model`    | `Model.new(provider:, name:, temperature:)` | (data — passed to an agent) |
| `Charter`  | `Charter.new(rules:, hooks:, skills:, commands:)` | (definition — intaken by a harness) |
| `Harness`  | `Harness.new(charter:, graph:)` | `.invoke` `.trigger` `.command` `.skill` (graph-backed) |
| `Agent`    | `Agent.new(model:, system:, harness:, tools:, memory:)` | `.run` `.ask` `.invoke` `.use` `.delegate` `.fan_out` |
| `Subagent` | `Subagent.new(name:, description:, model:, system:, tools:)` | `.run` · `.delegate` target |
| `Tool`     | `Tool.new(name:, description:, run:)` | model calls it during a run; `.run` directly |
| `Memory`   | `Memory.new` | `.remember(k,v)` `.recall(k)` `.forget` `.keys` |
| `Rule`     | `Rule.new(name:, text:)` | (data — folded into a charter) |
| `Skill`    | `Skill.new(name:, description:, instructions:)` | `.use` on an agent |
| `Hook`     | `Hook.new(on:, do:)` | (data — folded into a charter/agent) |
| `Command`  | `Command.new(name:, run:)` | `.run` `.call` `.invoke` |
| `Graph`    | `Graph.new(entry:, nodes:, edges:)` | `.invoke` `.trigger` `.run` |
| `Factory`  | `Factory.new(agent:, tasks:)` | `.push` `.run` `.size` |

The composition is **`Charter → Harness → Agent`**: a charter bundles governance,
a harness intakes a charter (and may carry a graph), and an **agent is a harness
plus a model**. Agents delegate to **subagents** (the Claude Code "agent"
primitive), call **tools**, and read/write **memory**.

### Model

Names a provider + model. Only `openai` is implemented today; the primitive
exists so other providers can be added without touching agent code. An agent's
`model:` accepts a model primitive or a bare string (openai shorthand).

```ruby
fast = Model.new(provider: "openai", name: "gpt-4o-mini", temperature: 0.2)
bot  = Agent.new(model: fast, system: "...")
```

### Agent (= harness + model)

```ruby
bot = Agent.new(
  name: "bot",
  model: "gpt-4o-mini",
  system: "You are helpful.",
  harness: gov,             # governance: rules/hooks/skills from its charter
  tools: [calculator],      # callables the model may invoke
  memory: notes,            # persistent store (remember/recall)
  subagents: [translator]   # delegates, reachable via .delegate
)

bot.invoke("...")                   # -> Message  (alias: .run / .ask)
bot.use(summarize, "...")           # run with a skill's instructions prepended
bot.delegate("translator", "...")   # hand off to a named subagent
bot.fan_out(["a", "b"])             # run over many inputs concurrently -> [Message]
```

Rules, hooks, and skills may also be passed inline (`rules:`, `hooks:`, `skills:`).

### Subagent

A named, described worker an agent delegates to.

```ruby
translator = Subagent.new(
  name: "translator",
  description: "Use to translate text into French",
  model: "gpt-4o-mini",
  system: "Translate to French. Output only the translation.",
  tools: [dictionary]
)
```

### Tool

A callable the model may invoke mid-run via OpenAI function calling. `run:` takes
the tool's string input. Attach with `tools:` on an agent or subagent; the run
loops until the model produces a final answer.

```ruby
shout = Tool.new(
  name: "shout",
  description: "Convert text to UPPERCASE. Use when asked to shout.",
  run: def (text) return text.upcase end
)

crier = Agent.new(tools: [shout])
crier.run("Please shout: hello")   # model calls shout -> HELLO
shout.run("direct")                # tools are callable directly -> DIRECT
```

### Memory

A persistent key/value store. Attach with `memory:` and the model gets built-in
`remember` and `recall` tools; `recall` fuzzily matches when a later turn phrases
a key differently.

```ruby
notes = Memory.new
bot = Agent.new(memory: notes, system: "Remember facts; recall before answering.")
bot.run("My favorite color is teal.")
bot.run("What is my favorite color?")   # -> teal
notes.recall("favorite_color")          # read it directly
```

### Charter + Harness

A **Charter** bundles rules, hooks, skills, and commands. A **Harness** intakes a
charter (and may carry a graph). A harness + a model is an agent; a harness with a
graph runs on its own.

```ruby
governance = Charter.new(rules: [concise], hooks: [redact], commands: [brief])
gov = Harness.new(charter: governance)
gov.command("brief").run("the sea")            # reach into the charter

assistant = Agent.new(harness: gov)
assistant.invoke("What is Rust good at?")      # rules + hooks applied

router = Harness.new(graph: flow)
router.trigger("some input")                   # graph-backed harness
```

### Graph

Control-flow routing. Nodes are agents, functions, or subgraphs; each node's
output feeds the next. An edge is a target node name or a router function that
returns the next name; `"end"` (or `nil`) stops.

```ruby
flow = Graph.new(
  entry: "classify",
  nodes: { classify: classifier, answer: responder },
  edges: {
    classify: def (msg) if msg.content.contains?("MATH") return "answer" end return "end" end,
    answer:   "end"
  }
)
flow.trigger("What is 2+2?")
```

### Factory (agent runner queue)

A worker agent plus a FIFO queue of tasks. Push tasks and drain them through the
worker, one result per task.

```ruby
runner = Factory.new(agent: worker, tasks: ["a", "b"])
runner.push("c")
runner.size            # 3
for r in runner.run    # drains the queue -> [Message]
  puts r.content
end
runner.run(["d", "e"]) # enqueue inline, then drain
```

## Processes & OTP

Underneath the agentic primitives is an **actor runtime**: lightweight processes
scheduled on **green threads** — cooperative and in-process. This is concurrency
(interleaved logical parallelism), not multiple CPU cores. It is how you build
**parallel, distributed-style** agent graphs: independent workers passing
messages, addressed by name, supervised and restarted on failure.

A process is `state + a handler`. `send`/`cast` enqueue and return immediately;
`receive`/`call`/`await` (and `drain()`) pump the scheduler until it is idle.

| Primitive     | Build / start | Invoke with |
|---------------|---------------|-------------|
| bare process  | `spawn(def (state, msg) … end, state)` | `send` · `pid.send` |
| `GenServer`   | `class X < GenServer` → `X.start(init)` | `.cast` `.call` `.stop` `.alive?` |
| `Supervisor`  | `Supervisor.start(children: [X.child(a)])` | `.which_children` |
| `Registry`    | `Registry.register(pid, name)` | `Registry.whereis(name)` |
| `Task`        | `Task.async(fn)` · `Task.parallel([fns])` | `Task.await(pid)` |

Free functions: `spawn` · `send(target, msg)` · `receive()` · `pid()` (current
process) · `monitor(pid)` · `drain()` · `raise(reason)` · `reply(value, state)`.
`send` and `monitor` take a pid **or** a registered name. A **`Pid`** is a value.

### Actors, the registry, and receive

```ruby
echo = spawn(def (state, msg)
  send(msg.get("from"), "echo: " + msg.get("text"))
  return state                       # handler returns the next state
end, nil)

Registry.register(echo, "echo")      # bind a name to the pid
send("echo", { from: pid(), text: "hi" })
puts receive()                       # echo: hi   (receive pumps the scheduler)
```

### GenServer

A stateful server with a Ruby class shape. `init` returns the starting state,
`handle_cast` handles fire-and-forget messages (returns the new state), and
`handle_call` handles request/reply with `reply(value, new_state)`.

```ruby
class Counter < GenServer
  def init(n)     return n end
  def handle_cast(msg, state)  return state + 1 end
  def handle_call(msg, state)  return reply(state, state) end
end

c = Counter.start(0)
c.cast("inc")
c.call("get")     # 1
```

### Supervision

`raise` crashes only the current process. `monitor(pid)` delivers a
`{ down: true, pid:, reason: }` message when a process dies. A `Supervisor`
watches its children and restarts a crashed one (any reason but `"normal"`),
re-running its `child` spec so it recovers with fresh state.

```ruby
sup = Supervisor.start({ children: [ Worker.child(42) ] })
worker = sup.which_children.first
worker.cast("crash")     # raises inside the worker
drain()                  # let the crash + restart run
sup.which_children.first # a new, healthy worker
```

### Task — green-thread fan-out

```ruby
results = Task.parallel([
  def () return worker.run("a").content end,
  def () return worker.run("b").content end
])                       # runs on green threads, joined in input order
```

## Custom workflows: classes, inheritance & composition

Primitives are classes, so higher-level abstractions are ordinary allegro
classes built on top of them — no new syntax per pattern. A class **subclasses**
a primitive (single inheritance), supplies a `config` (the base primitive's
construction hash), adds methods, and keeps state in `@ivars`. `Name.new` builds
it; `init` runs on construction; `self.base` reaches the underlying primitive;
undefined methods delegate to it.

```ruby
class Desk < Agent
  def config          # omit model: to inherit the MODEL env default
    return { system: "...", subagents: [translator], memory: notes }
  end

  def init            # runs on .new
    @handled = 0
  end

  def handle(text)    # domain method wrapping the inherited .invoke
    @handled = @handled + 1
    return self.invoke(text)
  end
end

desk = Desk.new
desk.handle("hello")

class TerseDesk < Desk               # class-to-class inheritance
  def handle(text)
    return self.invoke("In three words: " + text)
  end
end
```

### Composition: modules & delegation

Single inheritance picks one parent; **composition** covers the rest, Ruby-style.

A **module** is a bag of methods with no state of its own. `include` mixes it
into a class, where its methods run against the including instance's `@ivars`.
Method resolution is: the class's own methods, then included modules (last
`include` wins), then the parent chain.

```ruby
module Retryable
  def run_safe(text)
    return self.invoke(text)   # operates on the including instance
  end
end

class Desk < Agent
  include Retryable
end

Desk.new.run_safe("hello")
```

**Delegation** forwards named methods to a primitive held in an `@ivar` —
has-a composition without hand-written wrappers. `forward :method, to: @ivar`
sends `method` (and its args) to whatever that ivar holds.

```ruby
class Desk < Agent
  forward :remember, :recall, to: @notes   # delegate to the memory it owns

  def config
    return { system: "..." }
  end

  def init
    @notes = Memory.new
  end
end

d = Desk.new
d.remember("color", "teal")   # -> @notes.remember(...)
d.recall("color")             # -> teal
```

## Core data types

Primitives produce structured values, not bare strings:

- **`Message`** — an agent's output. `.content` / `.text`, `.role`, `.from`, `.length`.
  Prints as its content, so `puts msg` shows the text.
- **`HookResult`** — a hook's result: `.value`, `.halt?`. Built with `halt(v)` (stop) or
  `keep(v)` (continue).
- **`Pid`** — a live process. `.send` `.cast` `.call` `.stop` `.alive?` `.id`. Returned
  by `spawn`, `X.start`, `Supervisor.start`, `Task.async`, and `pid()`.

## Built-in functions

`puts` · `print` · `str` · `num` · `len` · `range(n)` / `range(a, b)` · `type_of(x)` ·
`fan_out(agent, list)` · `pipeline(input, agent, agent, …)` · `halt(v)` · `keep(v)` ·
`message(content, from)`

Process model: `spawn(fn, state)` · `send(target, msg)` · `receive()` · `pid()` ·
`monitor(pid)` · `drain()` · `raise(reason)` · `reply(value, state)`

`env` is a hash of environment variables: `env.MODEL`, `env["OPENAI_API_KEY"]`.

## Methods by type

- **string**: `upcase` `downcase` `strip` `length` `split(sep)` `contains?(s)` `to_s`
- **array**: `length` `first` `last` `push(x)` `reverse` `join(sep)` `get(i)`
- **hash**: `keys` `values` `get(k)` `set(k, v)` `has?(k)` · `h.KEY` reads a key
- **number**: `round` `floor` `ceil` `to_s`

## Examples

A guided progression, **basics → OTP → agentic → agentic + OTP**. Tiers 1–2 run
offline; the agentic tiers need `OPENAI_API_KEY`.

Basics — `01_hello` · `02_values` · `03_collections` · `04_control_flow` · `05_functions`

OTP — `06_processes` · `07_registry` · `08_genserver` · `09_supervisor` · `10_parallel`

Agentic — `11_agent` · `12_tools_memory` · `13_subagents` · `14_harness`

Agentic + OTP — `15_agent_pool` (parallel agent calls) · `16_supervised_agents`
(an Agent wrapped in a supervised GenServer) · `17_process_graph` (a pipeline of
agent processes addressed by name)

Fuller standalone demos:

- `examples/agents.al` — a worker class run through a Factory queue + fan-out
- `examples/harness.al` — Charter → Harness → Agent, hooks, graph, pattern matching
- `examples/workflow.al` — a Desk agent with a subagent, a tool, memory, and inheritance
