# Governance is a Charter; a Harness intakes it; an Agent is a harness + a
# model. The governed agent is defined inline as a class.

concise = Rule { name: "concise", text: "Answer in one short sentence." }

redact = Hook {
  on: "before_run",
  do: def (input)
    if input.contains?("password")
      return halt("(redacted for safety)")
    end
    return input
  end
}

brief = Command {
  name: "brief",
  run: def (topic) return "brief on: " + topic end
}

governance = Charter { rules: [concise], hooks: [redact], commands: [brief] }
gov = Harness { charter: governance }

class Assistant < Agent
  def config
    return { model: "gpt-4o-mini", temperature: 0.2, harness: gov }
  end
end

bot = Assistant.new
puts "reply: " + bot.invoke("What is Rust good at?").content

# the redact hook short-circuits the LLM entirely
puts "safe:  " + bot.invoke("my password is hunter2").content

# reach the charter's command through the harness
puts "cmd:   " + gov.command("brief").run("the ocean")

# A Graph is control-flow routing; a harness can carry one and run on its own.
classify = Agent { model: "gpt-4o-mini", system: "Reply with ONE word: QUESTION or STATEMENT.", temperature: 0.0 }

router = Harness {
  graph: Graph {
    entry: "classify",
    nodes: { classify: classify },
    edges: { classify: "end" }
  }
}

match router.trigger("Is the sky blue?").content.strip.upcase
when "QUESTION"
  puts "kind:  it's a question"
when "STATEMENT"
  puts "kind:  it's a statement"
when other
  puts "kind:  unknown (" + other + ")"
end
