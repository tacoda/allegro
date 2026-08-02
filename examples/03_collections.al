# 03 · Collections — arrays and hashes
#
# Arrays `[…]` and hashes `{ key: value }`. Arrays concatenate with `+`.

nums = [1, 2, 3]
nums.push(4)
puts nums.length
puts nums.first
puts nums.last
puts nums.reverse.join(", ")
more = nums + [5, 6]
puts more.join("-")

# hashes: string keys, dot-access for reads
person = { name: "Ada", role: "engineer" }
puts person.name
puts person.get("role")
person.set("city", "Austin")
puts person.keys.join(", ")
puts person.has?("city")
