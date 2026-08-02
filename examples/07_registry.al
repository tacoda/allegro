# 07 · Registry — addressing processes by name
#
# `Registry` binds a name to a pid so senders don't need to hold the pid — they
# address the work by name (location transparency). `send` takes a pid or a name.

echo = spawn(def (state, msg)
  send(msg.get("from"), "echo: " + msg.get("text"))
  return state
end, nil)

Registry.register(echo, "echo")

# send by name — the sender never sees the pid
send("echo", { from: pid(), text: "hello" })
puts receive()                       # echo: hello

puts Registry.whereis("echo")        # #<pid 1>
puts Registry.whereis("missing")     # nil
