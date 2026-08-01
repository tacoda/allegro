# Fan-out and pipeline over real agents.
# Run: OPENAI_API_KEY=... allegro run examples/agents.al

researcher = agent {
  name: "researcher",
  model: "gpt-4o-mini",
  system: "Give one interesting fact. One sentence.",
  temperature: 0.3
}

writer = agent {
  name: "writer",
  model: "gpt-4o-mini",
  system: "Rewrite the input as a single punchy tweet.",
  temperature: 0.7
}

# fan_out runs the researcher on every topic concurrently.
topics = ["octopuses", "the moon", "coffee"]
facts = fan_out(researcher, topics)

for f in facts
  puts "- " + f.content
end

# pipeline feeds one agent's output into the next.
tweet = pipeline("black holes", researcher, writer)
puts ""
puts "tweet: " + tweet
