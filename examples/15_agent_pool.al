# 15 · Agentic + OTP: parallel agent work
#
# Two ways to run an agent over many inputs at once:
#   • `agent.fan_out([...])` — true concurrent network calls (OS threads),
#     results in input order.
#   • `Task.parallel([...])` — green-thread fan-out over arbitrary work, so you
#     can compose agent calls with any other logic and join in order.
#
# Needs OPENAI_API_KEY.

worker = Agent.new(system: "Reply with a single emoji for the input.")

# concurrent network fan-out
for m in worker.fan_out(["cat", "rocket", "coffee"])
  puts m.content
end

# green-thread fan-out: each task does its own agent call, joined in order
labels = Task.parallel([
  def () return worker.run("dog").content end,
  def () return worker.run("pizza").content end
])

for label in labels
  puts label
end
