# AI primitives against OpenAI: Model, Tool, Agent, and a Harness that composes
# them into one runnable unit. Requires OPENAI_API_KEY (model + provider default
# from the MODEL / PROVIDER env vars, else gpt-4o-mini / openai).

model = Model.new(name: "gpt-4o-mini", temperature: 0.2)

concierge = Agent.new(
  model: model,
  system: "You are a terse hotel concierge. Use tools when asked about rooms.",
  tools: [
    Tool.new(
      name: "room_for",
      description: "Look up the room number for a department",
      run: fn dept -> "room 204" end
    )
  ]
)

# Bang variant: returns the bare message, or raises on error.
msg = "Which room is billing in?" |> Agent.run!(concierge)
IO.puts("concierge: #{msg.content}")

# Harness: wrap the agent (data-first pipeline) as one overridable unit.
harness = %Harness{run: fn input ->
  case Agent.run(input, concierge) do
    {:ok, m} -> {:ok, m.content}
    err -> err
  end
end}

{:ok, out} = "Greet a guest in one short sentence." |> Harness.run(harness)
IO.puts("harness: #{out}")
