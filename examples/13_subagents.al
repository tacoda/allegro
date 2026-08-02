# 13 · Subagents & graphs
#
# A Subagent is a named, described worker an agent delegates to. A Graph routes
# between nodes (agents or functions): each node's output feeds the next, and an
# edge is a target name or a router function that returns the next name.
#
# Needs OPENAI_API_KEY.

translator = Subagent.new(
  name: "translator",
  description: "Use to translate text into French",
  system: "Translate the input to French. Output only the translation."
)

desk = Agent.new(
  system: "You route work. Delegate translation to the translator.",
  subagents: [translator]
)

puts desk.delegate("translator", "Good morning")   # -> Bonjour

# A graph: classify, then answer only if it looks like math.
classifier = Agent.new(system: "Reply with exactly MATH or OTHER.")
responder  = Agent.new(system: "Answer the question concisely.")

flow = Graph.new(
  entry: "classify",
  nodes: { classify: classifier, answer: responder },
  edges: {
    classify: def (msg)
      if msg.content.contains?("MATH")
        return "answer"
      end
      return "end"
    end,
    answer: "end"
  }
)

puts flow.trigger("What is 2 + 2?")
