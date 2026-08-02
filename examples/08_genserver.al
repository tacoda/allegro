# 08 · GenServer — a stateful server object
#
# A GenServer is an OTP process with a Ruby class shape. `init` returns the
# starting state; `handle_cast` handles fire-and-forget messages (returns the
# new state); `handle_call` handles request/reply (`reply(value, new_state)`).
# `.start` spawns one and returns its pid; drive it with `.cast` and `.call`.

class Counter < GenServer
  def init(start)
    return start
  end

  def handle_cast(msg, state)
    if msg == "inc"
      return state + 1
    end
    return state
  end

  def handle_call(msg, state)
    return reply(state, state)   # reply with the count, keep it as state
  end
end

c = Counter.start(0)
c.cast("inc")
c.cast("inc")
c.cast("inc")
puts c.call("get")   # 3  (casts are drained before the synchronous call)
