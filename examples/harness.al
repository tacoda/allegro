# Build up definitions (rules, skills, hooks, commands), bundle them in a
# charter, then run them through a harness. Agents are constructors too; everything
# else is a plain constructor.

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

# --- charter: the definition, input to a harness ---
governance = charter {
  rules: [concise],
  skills: [summarize],
  hooks: [redact],
  commands: [brief]
}

# --- agent: a plain constructor now ---
assistant = agent {
  name: "assistant",
  model: "gpt-4o-mini",
  temperature: 0.2
}

# --- harness: ties an agent to a charter, then is invoked ---
h = harness { agent: assistant, charter: governance }

puts "reply: " + h.invoke("What is Rust good at?").content

# hooks short-circuit the LLM entirely
puts "safe:  " + h.invoke("my password is hunter2").content

# reach into the charter through the harness
puts "cmd:   " + h.command("brief").run("the ocean")

# --- graph: define steps, then trigger ---
classify = agent { name: "classify", model: "gpt-4o-mini", system: "Reply with ONE word: QUESTION or STATEMENT.", temperature: 0.0 }

flow = graph {
  entry: "classify",
  nodes: { classify: classify },
  edges: { classify: "end" }
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
