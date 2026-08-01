# A custom workflow defined inline as a class: an Agent (= harness + model)
# with a subagent to delegate to, a tool the model can call, and memory it
# keeps across turns. State lives in @ivars.

# A Subagent is a named delegate (the Claude Code "agent" primitive).
translator = Subagent {
  name: "translator",
  description: "Use to translate text into French",
  model: "gpt-4o-mini",
  system: "Translate the input to French. Output only the translation.",
  temperature: 0.0
}

# A Tool the model can call mid-run.
directory = Tool {
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

# Memory the agent reads and writes across turns.
notes = Memory { }

class Desk < Agent
  def config
    return {
      model: "gpt-4o-mini",
      system: "You are a concise front desk. Use the room_for tool for room questions. Whenever the visitor states a fact, you MUST call the remember tool to save it (do not just acknowledge). Before answering any question about the visitor, you MUST call recall first. One sentence.",
      subagents: [translator],
      tools: [directory],
      memory: notes
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
puts desk.handle("Please remember the VIP guest is named Dr. Lee.").content
puts desk.handle("Which room is the billing department in?").content
puts desk.to_french("Good morning, friend.").content
puts desk.handle("What is the VIP guest's name?").content
puts "handled: " + str(desk.count)

# Inheritance: specialize the workflow.
class TerseDesk < Desk
  def handle(text)
    return self.invoke("In three words: " + text)
  end
end

td = TerseDesk.new
puts td.handle("Describe a good cup of coffee.").content
