# Mutable state & named processes.
#
# Store is a cell for state that must outlive a single call (values are
# otherwise immutable). Process.register binds a name in the registry so
# messages can be addressed by name instead of pid.

counter = Store.new(0)
Store.update(counter, fn n -> n + 1 end)
Store.update(counter, fn n -> n + 1 end)
Store.put(counter, Store.get(counter) * 10)
IO.puts("store: #{Store.get(counter)}")

greeter = spawn(fn s, {:hi, from} ->
  send(from, {:reply, "hello from a named process"})
  {:noreply, s}
end, nil)

Process.register(greeter, :greeter)
send(:greeter, {:hi, self()})

receive do
  {:reply, msg} -> IO.puts("registry: #{msg}")
end
