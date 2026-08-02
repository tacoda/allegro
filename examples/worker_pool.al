# Worker pool & fan-out, both built on processes.
#
# Orchestrator.parallel runs each stage in its own process and gathers the
# results in order. Factory keeps a fixed pool of workers draining a job queue.

{:ok, fanned} = Orchestrator.parallel(10, [
  fn x -> x + 1 end,
  fn x -> x * 2 end,
  fn x -> x - 3 end
])

IO.puts("parallel: #{Enum.join(fanned, ", ")}")

{:ok, squared} = Factory.run([1, 2, 3, 4, 5, 6], fn x -> x * x end, 2)
IO.puts("factory (pool of 2): #{Enum.join(squared, ", ")}")
