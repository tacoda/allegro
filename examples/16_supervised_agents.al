# 16 · Agentic + OTP: a supervised agent worker
#
# Wrap an Agent inside a GenServer so it becomes a stateful, addressable,
# restartable process. The server holds the agent in an @ivar, counts the tasks
# it has handled, and answers `.call` requests. Put it under a Supervisor and a
# crash mid-task is isolated and restarted with a fresh agent.
#
# Needs OPENAI_API_KEY.

class Assistant < GenServer
  def init(role)
    @agent = Agent.new(system: "You are a " + role + ". Answer in one line.")
    return 0                      # state: number of tasks handled
  end

  def handle_call(task, handled)
    answer = @agent.run(task).content
    return reply(answer, handled + 1)
  end
end

sup = Supervisor.start({ children: [ Assistant.child("travel guide") ] })

bot = sup.which_children.first
puts bot.call("Best month to visit Lisbon?")
puts bot.call("And Kyoto?")

# if a worker ever crashed, the supervisor would restart it here; the address
# (via which_children / a Registry name) stays the entry point.
Registry.register(bot, "guide")
puts Registry.whereis("guide")
