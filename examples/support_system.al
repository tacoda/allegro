# A full-scale system: agents + OTP composed together.
#
# Each ticket flows through a two-stage agent pipeline (triage -> respond)
# assembled with Orchestrator.sequence and wrapped in a Harness — one runnable
# unit. The responder stage is wrapped in Retry so a transient failure recovers.
# A Store tallies throughput, and a `for` comprehension drives the batch.
#
# Requires OPENAI_API_KEY.

triage = Agent.new(
  system: "Classify the support ticket as one word — billing, tech, or other. Output only the word."
)

responder = Agent.new(
  system: "You are a terse support agent. Answer in one short sentence."
)

# The pipeline: triage tags the ticket, then the responder answers it. Each
# stage returns {:ok, out} | {:error, reason}, so the sequence short-circuits.
desk = %Harness{run: fn ticket ->
  Orchestrator.sequence(ticket, [
    fn t ->
      case Agent.run(t, triage) do
        {:ok, m} -> {:ok, "[#{String.trim(m.content)}] #{t}"}
        err -> err
      end
    end,
    fn tagged ->
      case Retry.run(fn -> Agent.run(tagged, responder) end, 3) do
        {:ok, m} -> {:ok, m.content}
        err -> err
      end
    end
  ])
end}

tickets = ["My card was charged twice this month", "The app crashes when I log in"]
handled = Store.new(0)

results = for ticket <- tickets do
  {:ok, answer} = Harness.run(ticket, desk)
  Store.update(handled, fn n -> n + 1 end)
  answer
end

Enum.each(results, fn r -> IO.puts("- #{r}") end)
IO.puts("handled #{Store.get(handled)} tickets")
