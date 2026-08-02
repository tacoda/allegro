# 05 · Functions
#
# `def name(params) … end` defines a function; `return` yields a value.
# `def (params) … end` with no name is an anonymous function — a value you can
# store, pass, and call. This is how hooks, tools, and process handlers are written.

def greet(name)
  return "hi, " + name
end

puts greet("Ada")

# an anonymous function is just a value
double = def (x) return x * 2 end
puts double(21)

# higher-order: take a function as an argument
def apply_twice(f, x)
  return f(f(x))
end

puts apply_twice(double, 5)   # 20

# recursion
def fact(n)
  if n <= 1
    return 1
  end
  return n * fact(n - 1)
end

puts fact(5)   # 120
