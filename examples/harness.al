# A charter is a bundle of governance (rules, hooks, skills, commands).
# A harness intakes a charter. An agent is a harness plus a model.

# --- charter parts: pure definitions ---
concise = rule { name: "concise", text: "Answer in one short sentence." }

summarize = skill {
  name: "summarize",
  description: "Condense text to its essence",
  instructions: "Summarize the following in one line:"
}

redact = hook {
  on: "before_run",
  do: def (input)
    if input.contains?("password")
      return halt("(redacted for safety)")
    end
    return input
  end
}

brief = command {
  name: "brief",
  description: "One-line brief",
  run: def (topic)
    return "brief on: " + topic
  end
}

# --- charter: the governance definition ---
governance = charter {
  rules: [concise],
  skills: [summarize],
  hooks: [redact],
  commands: [brief]
}

# --- harness: intakes the charter ---
gov = harness { charter: governance }

# --- agent: a harness plus a model ---
assistant = agent {
  name: "assistant",
  model: "gpt-4o-mini",
  temperature: 0.2,
  harness: gov
}

# invoking the agent applies the charter's rules and hooks
puts "reply: " + assistant.invoke("What is Rust good at?").content

# the redact hook short-circuits the LLM entirely
puts "safe:  " + assistant.invoke("my password is hunter2").content

# reach into the charter through the harness
puts "cmd:   " + gov.command("brief").run("the ocean")

# --- a harness can also carry a graph and run on its own ---
classify = agent { name: "classify", model: "gpt-4o-mini", system: "Reply with ONE word: QUESTION or STATEMENT.", temperature: 0.0 }

flow = harness {
  graph: graph {
    entry: "classify",
    nodes: { classify: classify },
    edges: { classify: "end" }
  }
}

label = flow.trigger("Is the sky blue?").content

match label.strip.upcase
when "QUESTION"
  puts "kind:  it's a question"
when "STATEMENT"
  puts "kind:  it's a statement"
when other
  puts "kind:  unknown (" + other + ")"
end
