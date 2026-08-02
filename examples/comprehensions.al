# `for` comprehensions: generators, filters, multiple (cartesian) generators,
# map iteration, and pattern-filtering. Each yields a new list; the bound
# variables are scoped to the comprehension.

# map a list
IO.inspect(for x <- [1, 2, 3, 4], do: x * x)

# filter, then map
IO.inspect(for x <- [1, 2, 3, 4], x > 2, do: x * x)

# two generators produce the cartesian product
IO.inspect(for x <- [1, 2], y <- [10, 20], do: {x, y})

# iterate a map as {key, value}
IO.inspect(for {k, v} <- %{a: 1, b: 2}, do: {k, v})

# a generator pattern that doesn't match filters the element out
oks = for {:ok, v} <- [{:ok, 1}, {:error, :nope}, {:ok, 3}], do: v
IO.inspect(oks)

# block form
IO.inspect(for n <- [1, 2, 3] do
  n + 100
end)
