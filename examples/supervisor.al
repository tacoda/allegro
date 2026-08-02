# Supervision: run children as monitored processes and restart them on crash.
#
# Each child is {id, start}. `start.(sup, id)` spawns a worker and triggers it;
# on success the worker sends {:ok, id, value} and stops :normal, on failure it
# crashes. A crashed child is restarted up to max_restarts, after which the
# supervisor reports {id, :error, reason}.

{:ok, results} = Supervisor.run([
  {:greeter, fn sup, id ->
     w = spawn(fn _s, :go -> send(sup, {:ok, id, "hello"}); {:stop, :normal} end, nil)
     send(w, :go)
     w
   end},
  {:flaky, fn sup, id ->
     w = spawn(fn _s, :go -> Kernel.boom() end, nil)
     send(w, :go)
     w
   end}
], 2)

Enum.each(results, fn r -> IO.inspect(r) end)
