# 09 · Supervision — crash isolation and restart
#
# `raise` crashes only the current process, not the program. `monitor(pid)`
# delivers a `{ down: true, pid:, reason: }` message when a process dies. A
# Supervisor watches its children and restarts a crashed one (reason != normal),
# re-running its child spec so it recovers with fresh state.

# --- crash isolation: a monitored process dies, the program continues ---
boom = spawn(def (state, msg)
  return raise("kaboom")
end, nil)

monitor(boom)
send(boom, "go")

down = receive()
puts "process died: " + down.get("reason")   # kaboom

# --- supervised restart ---
class Worker < GenServer
  def init(id)
    return id
  end

  def handle_cast(msg, state)
    return raise("worker crashed")   # any task crashes it
  end

  def handle_call(msg, state)
    return reply(state, state)
  end
end

sup = Supervisor.start({ children: [ Worker.child(42) ] })

original = sup.which_children.first
puts "before: " + str(original.call("id?"))   # 42

original.cast("do work")                       # crashes the worker (async)
drain()                                        # let the crash + restart run
restarted = sup.which_children.first
puts "restarted: " + str(original.id != restarted.id)   # true
puts "after:  " + str(restarted.call("id?"))            # 42 (fresh from the spec)
