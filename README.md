# allegro

A small Ruby-like language for composing **agents, harnesses, graphs, and
runner queues**. Build the definitions with capitalized constructors, then invoke
them.

```ruby
bot = Agent {
  model: env.MODEL || "gpt-4o-mini",
  system: "You are terse."
}

puts bot.run("Capital of France?").content   # => Paris.
```

Agents are backed by the OpenAI Chat Completions API. Set `OPENAI_API_KEY` in your
environment.

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

Primitives are **capitalized constructors** — call one with a config hash, keep the
result, and invoke it later with a method.

| Primitive  | Constructor | Invoke with |
|------------|-------------|-------------|
| `Model`    | `Model { provider:, name:, temperature: }` | (data — passed to an agent) |
| `Charter`  | `Charter { rules:, hooks:, skills:, commands: }` | (definition — intaken by a harness) |
| `Harness`  | `Harness { charter:, graph: }` | `.invoke` `.trigger` `.command` `.skill` (graph-backed) |
| `Agent`    | `Agent { model:, system:, harness:, tools:, memory: }` | `.run` `.ask` `.invoke` `.use` `.delegate` `.fan_out` |
| `Subagent` | `Subagent { name:, description:, model:, system:, tools: }` | `.run` · `.delegate` target |
| `Tool`     | `Tool { name:, description:, run: }` | model calls it during a run; `.run` directly |
| `Memory`   | `Memory { }` | `.remember(k,v)` `.recall(k)` `.forget` `.keys` |
| `Rule`     | `Rule { name:, text: }` | (data — folded into a charter) |
| `Skill`    | `Skill { name:, description:, instructions: }` | `.use` on an agent |
| `Hook`     | `Hook { on:, do: }` | (data — folded into a charter/agent) |
| `Command`  | `Command { name:, run: }` | `.run` `.call` `.invoke` |
| `Graph`    | `Graph { entry:, nodes:, edges: }` | `.invoke` `.trigger` `.run` |
| `Factory`  | `Factory { agent:, tasks: }` | `.push` `.run` `.size` |

The composition is **`Charter → Harness → Agent`**: a charter bundles governance,
a harness intakes a charter (and may carry a graph), and an **agent is a harness
plus a model**. Agents delegate to **subagents** (the Claude Code "agent"
primitive), call **tools**, and read/write **memory**.

### Model

Names a provider + model. Only `openai` is implemented today; the primitive
exists so other providers can be added without touching agent code. An agent's
`model:` accepts a model primitive or a bare string (openai shorthand).

```ruby
fast = Model { provider: "openai", name: "gpt-4o-mini", temperature: 0.2 }
bot  = Agent { model: fast, system: "..." }
```

### Agent (= harness + model)

```ruby
bot = Agent {
  name: "bot",
  model: "gpt-4o-mini",
  system: "You are helpful.",
  harness: gov,             # governance: rules/hooks/skills from its charter
  tools: [calculator],      # callables the model may invoke
  memory: notes,            # persistent store (remember/recall)
  subagents: [translator]   # delegates, reachable via .delegate
}

bot.invoke("...")                   # -> Message  (alias: .run / .ask)
bot.use(summarize, "...")           # run with a skill's instructions prepended
bot.delegate("translator", "...")   # hand off to a named subagent
bot.fan_out(["a", "b"])             # run over many inputs concurrently -> [Message]
```

Rules, hooks, and skills may also be passed inline (`rules:`, `hooks:`, `skills:`).

### Subagent

A named, described worker an agent delegates to.

```ruby
translator = Subagent {
  name: "translator",
  description: "Use to translate text into French",
  model: "gpt-4o-mini",
  system: "Translate to French. Output only the translation.",
  tools: [dictionary]
}
```

### Tool

A callable the model may invoke mid-run via OpenAI function calling. `run:` takes
the tool's string input. Attach with `tools:` on an agent or subagent; the run
loops until the model produces a final answer.

```ruby
shout = Tool {
  name: "shout",
  description: "Convert text to UPPERCASE. Use when asked to shout.",
  run: def (text) return text.upcase end
}

crier = Agent { model: "gpt-4o-mini", tools: [shout] }
crier.run("Please shout: hello")   # model calls shout -> HELLO
shout.run("direct")                # tools are callable directly -> DIRECT
```

### Memory

A persistent key/value store. Attach with `memory:` and the model gets built-in
`remember` and `recall` tools; `recall` fuzzily matches when a later turn phrases
a key differently.

```ruby
notes = Memory { }
bot = Agent { model: "gpt-4o-mini", memory: notes, system: "Remember facts; recall before answering." }
bot.run("My favorite color is teal.")
bot.run("What is my favorite color?")   # -> teal
notes.recall("favorite_color")          # read it directly
```

### Charter + Harness

A **Charter** bundles rules, hooks, skills, and commands. A **Harness** intakes a
charter (and may carry a graph). A harness + a model is an agent; a harness with a
graph runs on its own.

```ruby
governance = Charter { rules: [concise], hooks: [redact], commands: [brief] }
gov = Harness { charter: governance }
gov.command("brief").run("the sea")            # reach into the charter

assistant = Agent { model: "gpt-4o-mini", harness: gov }
assistant.invoke("What is Rust good at?")      # rules + hooks applied

router = Harness { graph: flow }
router.trigger("some input")                   # graph-backed harness
```

### Graph

Control-flow routing. Nodes are agents, functions, or subgraphs; each node's
output feeds the next. An edge is a target node name or a router function that
returns the next name; `"end"` (or `nil`) stops.

```ruby
flow = Graph {
  entry: "classify",
  nodes: { classify: classifier, answer: responder },
  edges: {
    classify: def (msg) if msg.content.contains?("MATH") return "answer" end return "end" end,
    answer:   "end"
  }
}
flow.trigger("What is 2+2?")
```

### Factory (agent runner queue)

A worker agent plus a FIFO queue of tasks. Push tasks and drain them through the
worker, one result per task.

```ruby
runner = Factory { agent: worker, tasks: ["a", "b"] }
runner.push("c")
runner.size            # 3
for r in runner.run    # drains the queue -> [Message]
  puts r.content
end
runner.run(["d", "e"]) # enqueue inline, then drain
```

## Custom workflows: classes & inheritance

Subclass any primitive to define a reusable workflow inline. A class supplies a
`config` (the base primitive's construction hash), adds methods, keeps state in
`@ivars`, and may override or extend inherited behavior. `Name.new` builds it;
`self.base` reaches the underlying primitive; undefined methods delegate to it.

```ruby
class Desk < Agent
  def config
    return { model: "gpt-4o-mini", system: "...", subagents: [translator], memory: notes }
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

## Core data types

Primitives produce structured values, not bare strings:

- **`Message`** — an agent's output. `.content` / `.text`, `.role`, `.from`, `.length`.
  Prints as its content, so `puts msg` shows the text.
- **`HookResult`** — a hook's result: `.value`, `.halt?`. Built with `halt(v)` (stop) or
  `keep(v)` (continue).

## Built-in functions

`puts` · `print` · `str` · `num` · `len` · `range(n)` / `range(a, b)` · `type_of(x)` ·
`fan_out(agent, list)` · `pipeline(input, agent, agent, …)` · `halt(v)` · `keep(v)` ·
`message(content, from)`

`env` is a hash of environment variables: `env.MODEL`, `env["OPENAI_API_KEY"]`.

## Methods by type

- **string**: `upcase` `downcase` `strip` `length` `split(sep)` `contains?(s)` `to_s`
- **array**: `length` `first` `last` `push(x)` `reverse` `join(sep)` `get(i)`
- **hash**: `keys` `values` `get(k)` `set(k, v)` `has?(k)` · `h.KEY` reads a key
- **number**: `round` `floor` `ceil` `to_s`

## Examples

- `examples/agents.al` — a worker class run through a Factory queue + fan-out
- `examples/harness.al` — Charter → Harness → Agent, hooks, graph, pattern matching
- `examples/workflow.al` — a Desk agent with a subagent, a tool, memory, and inheritance
