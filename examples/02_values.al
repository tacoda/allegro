# 02 · Values — numbers, booleans, nil
#
# Numbers are 64-bit floats but print as integers when whole. Booleans are
# `true`/`false`; absence is `nil`. `and`/`or`/`not` are the logical operators.

puts 7 * 6
puts 22 / 7
puts 22 % 7

# number methods (bind the receiver first — a paren-less `puts (x).m` would
# read as `puts(x).m`)
pi = 3.14159
puts pi.round
up = (2.1).ceil
puts up
down = (2.9).floor
puts down

# truthiness: everything but false and nil is truthy
puts true and not false
fallback = nil or "fallback"
puts fallback

# conversions
puts num("41") + 1
puts str(42) + "!"
