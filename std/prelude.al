# Allegro standard library — written in allegro, compiled into the binary and
# available to every program. Composition (Orchestrator, Factory) and OTP-lite
# (Supervisor, Retry) run on the actor scheduler: spawn/send/receive/monitor.

# ---------------------------------------------------------------------------
# Retry — run a 0-arity function that returns {:ok, v} | {:error, r}, retrying
# on error with exponential backoff until `max` attempts are exhausted.
# ---------------------------------------------------------------------------
defmodule Retry do
  def run(fun), do: run(fun, 3, 50)
  def run(fun, max), do: run(fun, max, 50)
  def run(fun, max, backoff), do: attempt(fun, max, backoff, 1)

  defp attempt(fun, max, backoff, n) do
    case fun.() do
      {:ok, v} ->
        {:ok, v}

      {:error, r} ->
        if n >= max do
          {:error, r}
        else
          Process.sleep(backoff * n)
          attempt(fun, max, backoff, n + 1)
        end
    end
  end
end

# ---------------------------------------------------------------------------
# Supervisor — run child specs {id, start} as monitored processes and restart
# them on crash (strategy :one_for_one). `start.(sup, id)` spawns a worker,
# triggers its work, and returns its pid; on success the worker sends
# {:ok, id, value} to `sup` and stops :normal, on failure it crashes (a DOWN).
# A crashed child is restarted up to `max_restarts`; the result is a list of
# {id, :ok, value} | {id, :error, reason}.
# ---------------------------------------------------------------------------
defmodule Supervisor do
  def run(children), do: run(children, 3)

  def run(children, max_restarts) do
    me = self()
    active = Enum.map(children, fn {id, start} ->
      pid = start.(me, id)
      monitor(pid)
      {pid, id, start, 0}
    end)
    loop(active, max_restarts, me, [])
  end

  defp loop([], _max, _me, results), do: {:ok, results}

  defp loop(active, max, me, results) do
    receive do
      {:ok, id, value} ->
        loop(remove_id(active, id), max, me, [{id, :ok, value} | results])

      {:DOWN, pid, :normal} ->
        loop(remove_pid(active, pid), max, me, results)

      {:DOWN, pid, reason} ->
        handle_crash(active, pid, reason, max, me, results)
    end
  end

  defp handle_crash(active, pid, reason, max, me, results) do
    case find_pid(active, pid) do
      nil ->
        loop(active, max, me, results)

      {_pid, id, start, restarts} ->
        rest = remove_pid(active, pid)

        if restarts < max do
          new_pid = start.(me, id)
          monitor(new_pid)
          loop([{new_pid, id, start, restarts + 1} | rest], max, me, results)
        else
          loop(rest, max, me, [{id, :error, reason} | results])
        end
    end
  end

  defp find_pid([], _pid), do: nil

  defp find_pid([{p, id, s, r} | t], pid) do
    if p == pid, do: {p, id, s, r}, else: find_pid(t, pid)
  end

  defp remove_pid(active, pid), do: Enum.reject(active, fn {p, _, _, _} -> p == pid end)
  defp remove_id(active, id), do: Enum.reject(active, fn {_, i, _, _} -> i == id end)
end

# ---------------------------------------------------------------------------
# Orchestrator — compose stage functions (each: input -> {:ok, out}|{:error,r}).
# `sequence` threads input through stages, short-circuiting on error.
# `parallel` fans every stage out to its own process, then gathers the results
# back through the mailbox (input order preserved).
# ---------------------------------------------------------------------------
defmodule Orchestrator do
  def sequence(input, []), do: {:ok, input}

  def sequence(input, [stage | rest]) do
    case stage.(input) do
      {:ok, out} -> sequence(out, rest)
      {:error, r} -> {:error, r}
    end
  end

  def parallel(input, stages) do
    me = self()
    fan_out(stages, input, me, 0)
    gather(length(stages), [])
  end

  defp fan_out([], _input, _me, _idx), do: :ok

  defp fan_out([stage | rest], input, me, idx) do
    worker = spawn(fn _s, :run ->
      send(me, {:result, idx, stage.(input)})
      {:stop, :normal}
    end, nil)
    send(worker, :run)
    fan_out(rest, input, me, idx + 1)
  end

  defp gather(0, acc) do
    ordered = acc |> Enum.sort_by(fn {i, _} -> i end) |> Enum.map(fn {_, v} -> v end)
    {:ok, ordered}
  end

  defp gather(n, acc) do
    receive do
      {:result, idx, val} -> gather(n - 1, [{idx, val} | acc])
    end
  end
end

# ---------------------------------------------------------------------------
# Factory — a fixed pool of persistent worker processes draining a job queue.
# `run(jobs, worker)` applies `worker.(job)` across the pool and returns the
# results in input order. A coordinator (this flow) hands each idle worker its
# next job on completion until the queue empties.
# ---------------------------------------------------------------------------
defmodule Factory do
  def run(jobs, worker), do: run(jobs, worker, 4)

  def run(jobs, worker, pool_size) do
    me = self()
    queue = index(jobs, 0)
    total = length(queue)
    workers = start_workers(cap(pool_size, total), worker, me)
    queue = dispatch(workers, queue)
    collect(queue, total, [])
  end

  defp collect(_queue, 0, acc) do
    ordered = acc |> Enum.sort_by(fn {i, _} -> i end) |> Enum.map(fn {_, v} -> v end)
    {:ok, ordered}
  end

  defp collect(queue, remaining, acc) do
    receive do
      {:done, worker, idx, result} ->
        queue = assign_next(worker, queue)
        collect(queue, remaining - 1, [{idx, result} | acc])
    end
  end

  defp start_workers(0, _worker, _me), do: []

  defp start_workers(n, worker, me) do
    w = spawn(fn _s, {:job, idx, job} ->
      send(me, {:done, self(), idx, worker.(job)})
      {:noreply, nil}
    end, nil)
    [w | start_workers(n - 1, worker, me)]
  end

  defp dispatch([], queue), do: queue
  defp dispatch([w | ws], queue), do: dispatch(ws, assign_next(w, queue))

  defp assign_next(_worker, []), do: []

  defp assign_next(worker, [{idx, job} | rest]) do
    send(worker, {:job, idx, job})
    rest
  end

  defp index([], _i), do: []
  defp index([h | t], i), do: [{i, h} | index(t, i + 1)]

  defp cap(n, max) do
    if n > max, do: max, else: n
  end
end

# ---------------------------------------------------------------------------
# Harness — a high-level, overridable runnable that wraps a run function
# (input -> {:ok, out} | {:error, r}). Compose agents/tools/orchestration into
# one unit; define your own by holding a run function.
# ---------------------------------------------------------------------------
defmodule Harness do
  defstruct [:run]

  def run(input, %Harness{run: f}), do: f.(input)
end

# ---------------------------------------------------------------------------
# Loop — an agentic run-until-done loop. `step` advances the state, `done?`
# decides when to stop; bounded by `max` iterations.
# ---------------------------------------------------------------------------
defmodule Loop do
  def run(state, step, done?), do: run(state, step, done?, 25)
  def run(state, step, done?, max), do: iterate(state, step, done?, max, 0)

  defp iterate(state, step, done?, max, n) do
    cond do
      done?.(state) -> {:ok, state}
      n >= max -> {:error, {:max_iterations, state}}
      true -> iterate(step.(state), step, done?, max, n + 1)
    end
  end
end

# ---------------------------------------------------------------------------
# StateGraph — a graph of nodes over a shared state map (LangGraph-flavored).
#
# * state    — a map; nodes read it and return a partial-update map.
# * nodes    — %{name => fn(state) -> update_map}.
# * edges    — %{name => next}, where `next` is a node name or a
#              fn(state) -> node name (conditional routing). `:end` stops.
# * reducers — %{key => fn(current, update) -> merged}; keys without a reducer
#              are overwritten. (e.g. an append reducer accumulates history.)
#
# Every node transition is checkpointed as %{node: name, state: state}; `run`
# returns {:ok, final_state, checkpoints}. `resume` restarts from any saved
# checkpoint, so a run can be inspected, replayed, or continued.
# ---------------------------------------------------------------------------
defmodule StateGraph do
  defstruct [:entry, :nodes, :edges, :reducers]

  def new(spec) do
    %StateGraph{
      entry: Map.get(spec, :entry),
      nodes: Map.get(spec, :nodes, %{}),
      edges: Map.get(spec, :edges, %{}),
      reducers: Map.get(spec, :reducers, %{})
    }
  end

  def run(graph, initial), do: run(graph, initial, 50)

  def run(%StateGraph{entry: entry} = graph, initial, max) do
    drive(graph, initial, entry, max, Store.new([]))
  end

  # Continue from a saved checkpoint: pick up at the edge out of its node.
  def resume(graph, checkpoint), do: resume(graph, checkpoint, 50)

  def resume(%StateGraph{} = graph, %{node: node, state: state}, max) do
    next = resolve_edge(Map.get(graph.edges, node), state)
    drive(graph, state, next, max, Store.new([]))
  end

  defp drive(_graph, state, :end, _fuel, checkpoints) do
    {:ok, state, Enum.reverse(Store.get(checkpoints))}
  end

  defp drive(_graph, state, _node, 0, checkpoints) do
    {:error, {:max_steps, state}, Enum.reverse(Store.get(checkpoints))}
  end

  defp drive(graph, state, node, fuel, checkpoints) do
    node_fn = Map.get(graph.nodes, node)
    state = merge(state, node_fn.(state), graph.reducers)
    Store.update(checkpoints, fn log -> [%{node: node, state: state} | log] end)
    next = resolve_edge(Map.get(graph.edges, node), state)
    drive(graph, state, next, fuel - 1, checkpoints)
  end

  # Merge a node's partial update into the state, per-key reducer or overwrite.
  defp merge(state, update, reducers) do
    Enum.reduce(Map.keys(update), state, fn acc, key ->
      merged = reduce_key(reducers, key, Map.get(acc, key), Map.get(update, key))
      Map.put(acc, key, merged)
    end)
  end

  defp reduce_key(reducers, key, current, update) do
    case Map.fetch(reducers, key) do
      {:ok, reducer} -> reducer.(current, update)
      :error -> update
    end
  end

  defp resolve_edge(edge, state) when is_function(edge), do: edge.(state)
  defp resolve_edge(edge, _state), do: edge
end
