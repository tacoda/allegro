# Custom workflows: subclass a primitive, supply a `config`, add methods, keep
# state in @ivars. Here an agent (= harness + model) delegates to a subagent.

# A subagent is a named delegate with a description of when to use it
# (the Claude Code "agent" primitive).
translator = subagent {
  name: "translator",
  description: "Use to translate text into French",
  model: "gpt-4o-mini",
  system: "Translate the input to French. Output only the translation.",
  temperature: 0.0
}

# A tool the model can call mid-run to look up a room by department.
directory = tool {
  name: "room_for",
  description: "Look up the room number for a department. Input is the department name.",
  run: def (dept)
    d = dept.downcase
    if d.contains?("billing")
      return "Room 204"
    elsif d.contains?("support")
      return "Room 118"
    else
      return "unknown department"
    end
  end
}

# A front desk: subclass `agent`, wire in the subagent and the tool, add methods.
class Desk < agent
  def config
    return {
      model: "gpt-4o-mini",
      system: "You are a concise front desk. Use the room_for tool for room questions. One sentence.",
      subagents: [translator],
      tools: [directory]
    }
  end

  def init
    @handled = 0
  end

  def handle(text)
    @handled = @handled + 1
    return self.invoke(text)
  end

  def to_french(text)
    return self.delegate("translator", text)
  end

  def count
    return @handled
  end
end

desk = Desk.new
puts desk.handle("What is the capital of France?").content
puts desk.to_french("Good morning, friend.").content
# the model calls the room_for tool to answer this one
puts desk.handle("Which room is the billing department in?").content
puts "handled: " + str(desk.count)

# Inheritance: specialize the workflow.
class TerseDesk < Desk
  def handle(text)
    return self.invoke("In three words: " + text)
  end
end

td = TerseDesk.new
puts td.handle("Describe the weather.").content
