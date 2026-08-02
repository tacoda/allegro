# Processes: spawn an actor, message it, receive replies, monitor for crashes.
# A process is `state + a handler`; the handler runs to completion per message
# and returns the new state. Only the top-level flow may `receive`.

defmodule Counter do
  def handle(n, {:inc, from}) do
    send(from, {:count, n + 1})
    {:noreply, n + 1}
  end
end

pid = spawn(Counter, 0)
send(pid, {:inc, self()})
send(pid, {:inc, self()})

receive do
  {:count, c} -> IO.puts("count is #{c}")
end

receive do
  {:count, c} -> IO.puts("count is #{c}")
end

# Monitor a process and observe its crash as a {:DOWN, pid, reason} message.
crasher = spawn(fn _state, _msg -> Kernel.boom() end, nil)
monitor(crasher)
send(crasher, :go)

receive do
  {:DOWN, who, reason} -> IO.puts("#{who} went down: #{reason}")
end

# `after` fires when the scheduler is idle (no message can still arrive).
receive do
  :never -> IO.puts("unreachable")
after 100 -> IO.puts("nothing came")
end
