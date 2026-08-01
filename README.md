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
| `model`   | `model { provider:, name:, temperature: }` | (data — passed to an agent) |
| `agent`   | `agent { model:, system:, … }` | `.run` `.ask` `.use` `.delegate` `.fan_out` |
| `rule`    | `rule { name:, text: }` | (data — folded into an agent/charter) |
| `skill`   | `skill { name:, description:, instructions: }` | `.use` on an agent |
| `hook`    | `hook { on:, do: }` | attached to an agent or charter |
| `command` | `command { name:, run: }` | `.run` `.call` `.invoke` |
| `charter` | `charter { rules:, hooks:, skills:, commands: }` | (definition — input to a harness) |
| `harness` | `harness { agent:, charter:, graph: }` | `.invoke` `.run` `.trigger` `.command` `.skill` |
| `graph`   | `graph { entry:, nodes:, edges: }` | `.invoke` `.trigger` `.run` |
| `factory` | `factory { build: }` | `.create` `.build` `.make` |

### model

Names a provider + model. Only `openai` is implemented today; the primitive
exists so other providers can be added without touching agent code. An agent's
`model:` accepts a model primitive or a bare string (openai shorthand).

```ruby
fast = model { provider: "openai", name: "gpt-4o-mini", temperature: 0.2 }
bot  = agent { model: fast, system: "..." }
```

### agent

```ruby
bot = agent {
  name: "bot",
  model: "gpt-4o-mini",
  system: "You are helpful.",
  temperature: 0.2,
  rules:  [concise],        # appended to the system prompt
  skills: [summarize],      # available via .use
  hooks:  [redact, loud],   # wrap every run
  agents: [translator]      # sub-agents, reachable via .delegate
}

bot.run("...")                      # -> Message
bot.use(summarize, "...")           # run with a skill's instructions prepended
bot.delegate("translator", "...")   # hand off to a named sub-agent
bot.fan_out(["a", "b"])             # run over many inputs concurrently -> [Message]
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
ties a charter to an agent (or a graph) and runs it — applying the charter's rules
and hooks around each invocation.

```ruby
governance = charter {
  rules:    [concise],
  hooks:    [redact],
  skills:   [summarize],
  commands: [brief]
}

h = harness { agent: bot, charter: governance }
h.invoke("What is Rust good at?")   # rules + hooks applied
h.command("brief").run("the sea")   # reach into the charter
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
class ReviewFlow < harness
  def config
    return { agent: reviewer, charter: house_rules }
  end

  def init            # runs on .new
    @reviews = 0
  end

  def review(text)    # domain method wrapping the inherited .invoke
    @reviews = @reviews + 1
    return self.invoke("Review this: " + text)
  end
end

flow = ReviewFlow.new
flow.review("let x = 1")

class StrictReview < ReviewFlow      # class-to-class inheritance
  def review(text)
    return self.invoke("Be harsh. " + text)
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
