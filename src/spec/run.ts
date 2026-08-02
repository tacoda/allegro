import { Agent, Tool, Memory, Subagent, Graph, type GraphNode } from "../agents/index.ts";
import { Supervisor, child, runtime, type Runtime } from "../otp/index.ts";
import type { System, SystemDefinition, SystemSpec, GraphSpec } from "./define.ts";

const entries = <V>(o: Record<string, V> | undefined): [string, V][] => Object.entries(o ?? {});

// Instantiate and wire every primitive a spec declares, in dependency order, and
// return the System handle. Does not call the spec's `run`.
export async function buildSystem(spec: SystemSpec, rt: Runtime = runtime): Promise<System> {
  const memory = buildMemory(spec);
  const tools = buildTools(spec);
  const subagents = buildSubagents(spec, tools);
  const agents = buildAgents(spec, { tools, subagents, memory, rt });
  const graphs = buildGraphs(spec, agents, subagents);
  const supervisors = await buildSupervisors(spec, rt);

  return {
    tools,
    memory,
    subagents,
    agents,
    graphs,
    supervisors,
    servers: spec.servers ?? {},
    start: (server, ...args) => server.start(...args),
    runtime: rt,
  };
}

function buildMemory(spec: SystemSpec): Record<string, Memory> {
  return fromEntries(entries(spec.memory), (seed) => new Memory(Object.keys(seed).length ? seed : undefined));
}

function buildTools(spec: SystemSpec): Record<string, Tool> {
  return fromEntries(entries(spec.tools), (cfg, name) => new Tool({ name, description: cfg.description, run: cfg.run }));
}

function buildSubagents(spec: SystemSpec, tools: Record<string, Tool>): Record<string, Subagent> {
  return fromEntries(
    entries(spec.subagents),
    (cfg, name) =>
      new Subagent({ name, description: cfg.description, model: cfg.model, system: cfg.system, tools: pick(tools, cfg.tools) }),
  );
}

interface AgentDeps {
  tools: Record<string, Tool>;
  subagents: Record<string, Subagent>;
  memory: Record<string, Memory>;
  rt: Runtime;
}

function buildAgents(spec: SystemSpec, deps: AgentDeps): Record<string, Agent> {
  return fromEntries(
    entries(spec.agents),
    (cfg, name) =>
      new Agent(
        {
          name,
          model: cfg.model,
          system: cfg.system,
          temperature: cfg.temperature,
          tools: pick(deps.tools, cfg.tools),
          memory: cfg.memory ? deps.memory[cfg.memory] : undefined,
          subagents: pick(deps.subagents, cfg.subagents),
        },
        deps.rt,
      ),
  );
}

function buildGraphs(
  spec: SystemSpec,
  agents: Record<string, Agent>,
  subagents: Record<string, Subagent>,
): Record<string, Graph> {
  return fromEntries(
    entries(spec.graphs),
    (g) => new Graph({ entry: g.entry, nodes: resolveNodes(g, agents, subagents), edges: g.edges }),
  );
}

async function buildSupervisors(spec: SystemSpec, rt: Runtime): Promise<System["supervisors"]> {
  const supervisors: System["supervisors"] = {};
  for (const [name, s] of entries(spec.supervisors)) {
    const children = s.children.map((c) => ("server" in c ? child(c.server, ...(c.args ?? [])) : child(c)));
    supervisors[name] = await Supervisor.start({ strategy: s.strategy, maxRestarts: s.maxRestarts, children }, rt);
  }
  return supervisors;
}

function pick<T>(all: Record<string, T>, names?: string[]): T[] {
  return (names ?? []).map((n) => all[n]).filter(Boolean) as T[];
}

function fromEntries<V, T>(pairs: [string, V][], make: (value: V, name: string) => T): Record<string, T> {
  const out: Record<string, T> = {};
  for (const [name, value] of pairs) out[name] = make(value, name);
  return out;
}

// Build the system, then run the spec's entrypoint.
export async function runSystem(def: SystemDefinition, rt: Runtime = runtime): Promise<System> {
  const sys = await buildSystem(def.spec, rt);
  if (def.spec.run) await def.spec.run(sys);
  return sys;
}

// Load a spec: a .ts/.js module (default export is a SystemDefinition) or a JSON
// file (the behavior-free data subset).
export async function loadDefinition(path: string): Promise<SystemDefinition> {
  if (path.endsWith(".json")) {
    const spec = JSON.parse(await Bun.file(path).text()) as SystemSpec;
    return { spec };
  }
  const mod = await import(path.startsWith("/") ? path : `${process.cwd()}/${path}`);
  if (!mod.default) throw new Error(`${path} has no default export (use \`export default defineSystem(...)\`)`);
  return mod.default as SystemDefinition;
}

function resolveNodes(
  g: GraphSpec,
  agents: Record<string, Agent>,
  subagents: Record<string, Subagent>,
): Record<string, GraphNode> {
  const nodes: Record<string, GraphNode> = {};
  for (const [name, node] of Object.entries(g.nodes)) {
    if (typeof node === "string") {
      const resolved = agents[node] ?? subagents[node];
      if (!resolved) throw new Error(`graph node '${name}' references unknown agent '${node}'`);
      nodes[name] = resolved;
    } else {
      nodes[name] = node;
    }
  }
  return nodes;
}
