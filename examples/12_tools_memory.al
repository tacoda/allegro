# 12 · Tools & memory
#
# A Tool is a callable the model may invoke mid-run (OpenAI function calling);
# `run:` is a function of the tool's string input. A Memory is a persistent
# key/value store — attach it and the model gets built-in remember/recall tools.
#
# Needs OPENAI_API_KEY.

shout = Tool.new(
  name: "shout",
  description: "Convert text to UPPERCASE. Use when asked to shout.",
  run: def (text) return text.upcase end
)

notes = Memory.new

assistant = Agent.new(
  system: "Use your tools. Remember facts and recall them before answering.",
  tools: [shout],
  memory: notes
)

puts assistant.run("Please shout: hello")     # model calls shout -> HELLO
puts shout.run("direct call")                 # tools are callable directly -> DIRECT CALL

assistant.run("My favorite color is teal.")
puts assistant.run("What is my favorite color?")   # -> teal
puts notes.recall("favorite_color")                # read the store directly
