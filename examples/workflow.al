# Custom workflows are defined by subclassing a primitive. A class supplies a
# `config` for the base primitive, adds methods, and keeps state in @ivars.

# A reusable review harness: subclass `harness`, give it a charter + agent,
# and add a domain method.
class ReviewFlow < harness
  def config
    reviewer = agent {
      name: "reviewer",
      model: "gpt-4o-mini",
      system: "You review text. Reply with one concrete suggestion.",
      temperature: 0.2
    }
    rules = charter { rules: [ rule { name: "short", text: "One sentence." } ] }
    return { agent: reviewer, charter: rules }
  end

  def init
    @reviews = 0
  end

  # Domain method: wraps the inherited `invoke`.
  def review(text)
    @reviews = @reviews + 1
    return self.invoke("Review this: " + text)
  end

  def count
    return @reviews
  end
end

flow = ReviewFlow.new
puts flow.review("fn add(a,b){a+b}").content
puts flow.review("let x = 1").content
puts "reviews run: " + str(flow.count)

# Inheritance: specialize the workflow further.
class StrictReview < ReviewFlow
  def review(text)
    return self.invoke("Be harsh. Review this: " + text)
  end
end

strict = StrictReview.new
puts strict.review("x = x").content
