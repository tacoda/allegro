# StateGraph: a graph of nodes over a shared state map, with checkpointing.
#
# Two nodes — :work does the step, :check routes — with an append reducer that
# accumulates history. Every transition is checkpointed, so a run can be
# inspected and resumed from any point.

graph = StateGraph.new(%{
  entry: :work,
  nodes: %{
    work: fn s ->
      n = Map.get(s, :tries) + 1
      %{tries: n, history: ["attempt #{n}"]}
    end,
    check: fn _s -> %{} end
  },
  edges: %{
    work: :check,
    check: fn s -> if Map.get(s, :tries) >= 3, do: :end, else: :work end
  },
  reducers: %{
    history: fn current, update -> current ++ update end
  }
})

{:ok, final, checkpoints} = StateGraph.run(graph, %{tries: 0, history: []})

IO.puts("tries: #{Map.get(final, :tries)}")
IO.puts("history: #{Enum.join(Map.get(final, :history), " -> ")}")
IO.puts("checkpoints saved: #{length(checkpoints)}")

# Resume from the first checkpoint — replays forward to the same result.
{:ok, resumed, _} = StateGraph.resume(graph, hd(checkpoints))
IO.puts("resumed history: #{Enum.join(Map.get(resumed, :history), " -> ")}")
