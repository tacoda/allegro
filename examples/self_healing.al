# Self-healing supervision: a flaky child that recovers after retries.
#
# The child crashes on its first two attempts and succeeds on the third. A
# Store counter, captured by the start closure, persists across restarts — so
# the supervisor's restarts make forward progress instead of looping forever.

tries = Store.new(0)

{:ok, results} = Supervisor.run([
  {:flaky, fn sup, id ->
     w = spawn(fn _s, :go ->
       n = Store.update(tries, fn t -> t + 1 end)
       if n < 3 do
         Kernel.boom()
       else
         send(sup, {:ok, id, "recovered on attempt #{n}"})
         {:stop, :normal}
       end
     end, nil)
     send(w, :go)
     w
   end}
], 5)

Enum.each(results, fn r -> IO.inspect(r) end)
