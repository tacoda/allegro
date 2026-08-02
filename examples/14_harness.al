# 14 · Charter → Harness → Agent
#
# Governance composes: a Charter bundles rules, hooks, and commands; a Harness
# intakes a charter; an Agent is a harness plus a model. Primitives are classes,
# so you can subclass Agent to add domain methods while inheriting `.invoke`.
#
# Needs OPENAI_API_KEY.

concise = Rule.new(name: "concise", text: "Answer in one sentence.")
redact  = Hook.new(on: "after", do: def (out)
  return keep(out)   # a real hook might scrub the output here
end)

governance = Charter.new(rules: [concise], hooks: [redact])
gov = Harness.new(charter: governance)

assistant = Agent.new(harness: gov, system: "You are helpful.")
puts assistant.invoke("What is Rust good at?")   # rules + hooks applied

# Subclass Agent: inherit .invoke, add a domain method and per-instance state.
class Desk < Agent
  def config
    return { system: "You are a support desk. Be kind and brief." }
  end

  def init
    @handled = 0
  end

  def handle(text)
    @handled = @handled + 1
    return self.invoke(text)
  end
end

desk = Desk.new
puts desk.handle("My order is late.")
