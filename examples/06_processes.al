# 06 · Processes — the actor model
#
# A process is `state + a handler`, scheduled on green threads (cooperative,
# in-process). `spawn(handler, state)` starts one and returns its pid; the
# handler is `def (state, msg) … return new_state end`. `send` delivers a
# message; only the top-level flow `receive`s. `pid()` is the current process.

# A counter process: each :inc bumps its state and reports back to the sender.
counter = spawn(def (n, msg)
  send(msg.get("reply_to"), n + 1)
  return n + 1
end, 0)

send(counter, { reply_to: pid() })
send(counter, { reply_to: pid() })

# receive pumps the scheduler until a message arrives for us
puts "count = " + str(receive())   # 1
puts "count = " + str(receive())   # 2
