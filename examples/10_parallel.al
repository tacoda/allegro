# 10 · Parallel — green-thread fan-out
#
# `Task.async(fn)` runs a function on its own green thread and returns a pid;
# `Task.await(pid)` blocks (pumps the scheduler) until it finishes and yields the
# result. `Task.parallel([fns])` fans a list out and joins them in input order.
#
# Green threads are cooperative and in-process: this is interleaved concurrency,
# not multiple CPU cores.

# await a single task
t = Task.async(def () return 6 * 7 end)
puts Task.await(t)   # 42

# fan several out, collect in order
results = Task.parallel([
  def () return 1 + 1 end,
  def () return 2 * 3 end,
  def () return 10 - 4 end
])

for r in results
  puts r             # 2, 6, 6
end
