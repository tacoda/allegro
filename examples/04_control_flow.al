# 04 · Control flow — if, while, for, match
#
# `if/elsif/else/end`, `while`, `for … in`, and `match` for dispatch on a value
# by literal, type, or binding.

x = 7
if x < 5
  puts "small"
elsif x < 10
  puts "medium"
else
  puts "large"
end

n = 0
while n < 3
  puts "tick " + str(n)
  n = n + 1
end

for i in range(1, 4)
  puts "row " + str(i)      # 1 2 3
end

reply = "yes"
match reply
when "yes"          # literal
  puts "affirmative"
when Number         # type
  puts "a number"
when other          # bare name binds the value
  puts "got: " + str(other)
end
