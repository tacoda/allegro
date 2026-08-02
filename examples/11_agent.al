# 11 · Agent — the core agentic primitive
#
# An Agent is an LLM plus the harness wrapped around it. `.new` takes keyword
# config; `model:` defaults to the MODEL env var (else gpt-4o-mini). `.run`
# (alias `.ask`) returns a Message — a structured value, not a bare string.
#
# Needs OPENAI_API_KEY in the environment to actually call the model.

bot = Agent.new(system: "You are terse. Answer in one word.")

reply = bot.run("Capital of France?")
puts reply.content     # Paris
puts reply.role        # assistant
puts reply.from        # agent

# a Message prints as its content
puts bot.ask("Capital of Japan?")
