# Define a worker agent inline as a class, then run a queue of tasks through a
# Factory (an agent runner queue).
# Run: OPENAI_API_KEY=... allegro run examples/agents.al

class Researcher < Agent
  def config
    return {
      model: "gpt-4o-mini",
      system: "Give one interesting fact about the topic. One sentence.",
      temperature: 0.3
    }
  end
end

# A Factory queues tasks and drains them through the worker, one result each.
runner = Factory {
  agent: Researcher.new,
  tasks: ["octopuses", "the moon"]
}
runner.push("coffee")

for fact in runner.run
  puts "- " + fact.content
end

# fan_out runs a worker over inputs concurrently.
tweets = fan_out(Researcher.new, ["black holes", "the deep sea"])
for t in tweets
  puts "* " + t.content
end
