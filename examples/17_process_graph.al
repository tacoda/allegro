# 17 · Agentic + OTP: a pipeline of agent processes
#
# Each stage of the pipeline is its own process holding an agent in its state,
# addressed by name through the Registry (location transparency — a stage sends
# to "responder", never to a pid). Messages flow classifier -> responder -> you.
# This is how you wire agents into a parallel, distributed-style graph.
#
# Needs OPENAI_API_KEY.

# stage 2: answer, tagged with the category the classifier attached
responder = spawn(def (state, msg)
  answer = state.get("agent").run(msg.get("text")).content
  send(msg.get("reply_to"), answer)
  return state
end, { agent: Agent.new(system: "Answer in one sentence.") })

# stage 1: classify, then forward to the responder by name
classifier = spawn(def (state, msg)
  label = state.get("agent").run(msg.get("text")).content
  send("responder", {
    text: msg.get("text") + " [" + label + "]",
    reply_to: msg.get("reply_to")
  })
  return state
end, { agent: Agent.new(system: "Reply with one category word.") })

Registry.register(responder, "responder")
Registry.register(classifier, "classifier")

# kick off the pipeline; receive pumps every stage to completion
send("classifier", { text: "What is the tallest mountain on Earth?", reply_to: pid() })
puts receive()
