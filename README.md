# allegro

A small Ruby-like language for composing **agents, harnesses, graphs, and factories**.
Build the definitions with plain constructors, then invoke them.

```ruby
bot = agent {
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
OPENAI_API_KEY=sk-... ./target/release/allegro run examples/harness.al
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
when Number       # type: Nil Bool Number String Array Hash Message Agent Harness ...
  puts "a number"
when other        # a bare name binds the value
  puts "got: " + str(other)
end
```

### Anonymous functions

`def (params) ... end` is a value — used for hooks, commands, factories, edges:

```ruby
double = def (x) return x * 2 end
puts double(21)   # 42
```

## Primitives

Everything is a plain constructor — call it with a config hash, keep the result,
and invoke it later with a method. Nothing needs a special keyword.

| Primitive | Constructor | Invoke with |
|-----------|-------------|-------------|
| `model`    | `model { provider:, name:, temperature: }` | (data — passed to an agent) |
| `charter`  | `charter { rules:, hooks:, skills:, commands: }` | (definition — intaken by a harness) |
| `harness`  | `harness { charter:, graph: }` | `.invoke` `.trigger` `.command` `.skill` (graph-backed) |
| `agent`    | `agent { model:, system:, harness: }` | `.run` `.ask` `.invoke` `.use` `.delegate` `.fan_out` |
| `subagent` | `subagent { name:, description:, model:, system:, tools: }` | `.run` `.delegate`-target |
| `tool`     | `tool { name:, description:, run: }` | model calls it during a run; `.run` directly |
| `rule`     | `rule { name:, text: }` | (data — folded into a charter) |
| `skill`    | `skill { name:, description:, instructions: }` | `.use` on an agent |
| `hook`     | `hook { on:, do: }` | (data — folded into a charter/agent) |
| `command`  | `command { name:, run: }` | `.run` `.call` `.invoke` |
| `graph`    | `graph { entry:, nodes:, edges: }` | `.invoke` `.trigger` `.run` |
| `factory`  | `factory { build: }` | `.create` `.build` `.make` |

The composition is **`charter → harness → agent`**: a charter bundles governance,
a harness intakes a charter (and may carry a graph), and an **agent is a harness
plus a model**. Agents delegate to **subagents** — named, described worker agents
(the Claude Code "agent" primitive).

### model

Names a provider + model. Only `openai` is implemented today; the primitive
exists so other providers can be added without touching agent code. An agent's
`model:` accepts a model primitive or a bare string (openai shorthand).

```ruby
fast = model { provider: "openai", name: "gpt-4o-mini", temperature: 0.2 }
bot  = agent { model: fast, system: "..." }
```

### agent (= harness + model)

An agent combines a model with a harness (its governance) and can delegate to
subagents.

```ruby
bot = agent {
  name: "bot",
  model: fast,              # a model primitive or a string
  system: "You are helpful.",
  temperature: 0.2,
  harness: gov,             # governance: rules/hooks/skills from its charter
  subagents: [translator]   # delegates, reachable via .delegate
}

bot.invoke("...")                   # -> Message  (alias: .run / .ask)
bot.use(summarize, "...")           # run with a skill's instructions prepended
bot.delegate("translator", "...")   # hand off to a named subagent
bot.fan_out(["a", "b"])             # run over many inputs concurrently -> [Message]
```

Rules, hooks, and skills can also be passed inline (`rules:`, `hooks:`, `skills:`)
instead of through a harness — they end up in the same place.

### subagent

A named, described worker an agent delegates to.

```ruby
translator = subagent {
  name: "translator",
  description: "Use to translate text into French",
  model: "gpt-4o-mini",
  system: "Translate to French. Output only the translation.",
  tools: [dictionary]        # subagents (and agents) can carry tools
}
```

### tool

A callable the model may invoke mid-run via OpenAI function calling. `run:` is a
function taking the tool's string input. Attach with `tools:` on an agent or
subagent; the run loops until the model produces a final answer.

```ruby
shout = tool {
  name: "shout",
  description: "Convert text to UPPERCASE. Use when asked to shout.",
  run: def (text) return text.upcase end
}

crier = agent { model: "gpt-4o-mini", system: "Call shout when asked.", tools: [shout] }
crier.run("Please shout: hello")   # model calls shout, result fed back -> HELLO
shout.run("direct")                # tools are also callable directly -> DIRECT
```

### rule / skill / hook / command

```ruby
concise   = rule  { name: "concise", text: "Answer in one sentence." }
summarize = skill { name: "summarize", description: "Condense", instructions: "Summarize:" }

redact = hook {
  on: "before_run",              # or "after_run"
  do: def (input)
    if input.contains?("password")
      return halt("(redacted)")  # halt short-circuits the run
    end
    return input
  end
}

brief = command { name: "brief", run: def (topic) return "brief: " + topic end }
brief.run("the ocean")
```

### charter + harness

A **charter** is a pure bundle of rules, hooks, skills, and commands. A **harness**
intakes a charter (and may carry a graph). Give a harness a model and you have an
agent; give it a graph and it runs on its own.

```ruby
governance = charter {
  rules:    [concise],
  hooks:    [redact],
  skills:   [summarize],
  commands: [brief]
}

gov = harness { charter: governance }
gov.command("brief").run("the sea")   # reach into the charter

# a harness + a model = an agent
assistant = agent { model: "gpt-4o-mini", harness: gov }
assistant.invoke("What is Rust good at?")   # rules + hooks applied

# a harness with a graph is invocable on its own
router = harness { graph: flow }
router.trigger("some input")
```

### graph

Nodes are agents, functions, or **subgraphs**. Each node's output becomes the next
node's input. An edge is either a target node name or a router **function** that
returns the next name; `"end"` (or `nil`) stops.

```ruby
flow = graph {
  entry: "classify",
  nodes: { classify: classifier, answer: responder },
  edges: {
    classify: def (msg) if msg.content.contains?("MATH") return "answer" end return "end" end,
    answer:   "end"
  }
}
flow.trigger("What is 2+2?")
```

### factory

```ruby
persona = factory {
  build: def (voice)
    return agent { name: voice, model: "gpt-4o-mini", system: "Speak like a " + voice + "." }
  end
}
pirate = persona.create("pirate")
```

## Custom workflows: classes & inheritance

Subclass any primitive to define your own reusable workflow. A class supplies a
`config` (the base primitive's construction hash), adds methods, keeps state in
`@ivars`, and may override or extend inherited behavior. `Name.new` builds it;
`self.base` reaches the underlying primitive; undefined methods delegate to it.

```ruby
class Desk < agent
  def config
    return { model: "gpt-4o-mini", system: "...", subagents: [translator] }
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

- `examples/agents.al` — fan-out + pipeline
- `examples/harness.al` — charter + harness, hooks, graph, pattern matching
- `examples/workflow.al` — custom workflows via classes and inheritance
