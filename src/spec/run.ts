import { Agent, Tool, Memory, Graph, type GraphNode, type GraphEdge } from "../agents/index.ts";
import { expandMcp } from "../agents/mcp.ts";
import { bus } from "../runtime/bus.ts";
import type { System, SystemDefinition, SystemSpec, SpecNode, Transitions, HookEvent } from "./define.ts";

// Instantiate every node a spec declares and wire the edges, returning the
// System handle. Does not call the spec's `run`.
export async function buildSystem(spec: SystemSpec): Promise<System> {
  bus.clearHooks();
  for (const [event, hook] of Object.entries(spec.hooks ?? {})) {
    bus.register(event as HookEvent, hook!);
  }

  const scope = await buildScope(spec.nodes, spec.transitions);

  const system: System = {
    nodes: scope.all,
    tools: scope.tools,
    memory: scope.memory,
    agents: scope.agents,
    graphs: scope.graphs,
    graph: scope.graph,
    run: (input) => scope.graph.trigger(input),
    command: (name, input) => runCommand(spec, scope.graph, name, input),
  };
  return system;
}

interface Scope {
  graph: Graph;
  all: Record<string, unknown>;
  tools: Record<string, Tool>;
  memory: Record<string, Memory>;
  agents: Record<string, Agent>;
  graphs: Record<string, Graph>;
}

interface Skill {
  instructions: string;
  uses: string[];
}

interface ResolveCtx {
  tools: Record<string, Tool>;
  mcpTools: Record<string, Tool[]>;
  memory: Record<string, Memory>;
  skills: Record<string, Skill>;
  agents: Record<string, Agent>;
}

// Build one nodes+transitions scope into a runnable Graph. Recurses for nested
// `type:"graph"` nodes. Resources (tool/skill/memory/mcp) become dependencies;
// flow nodes (agent/fn/graph) become the graph's routable nodes.
async function buildScope(specNodes: Record<string, SpecNode>, transitions: Transitions): Promise<Scope> {
  const ctx = await buildResources(specNodes);
  buildAgentShells(specNodes, ctx.agents); // pass 1: so delegation can name any agent
  wireAgents(specNodes, ctx); // pass 2: resolve `uses`
  const { flow, graphs } = await buildFlow(specNodes, ctx.agents);

  const { entry, ...edges } = transitions;
  const graph = new Graph({ entry, nodes: flow, edges: edges as Record<string, GraphEdge> });
  const all: Record<string, unknown> = { ...ctx.tools, ...ctx.memory, ...ctx.agents, ...graphs };
  return { graph, all, tools: ctx.tools, memory: ctx.memory, agents: ctx.agents, graphs };
}

// Resources: tools, memory, skills, mcp (async expand). Agents added later.
async function buildResources(specNodes: Record<string, SpecNode>): Promise<ResolveCtx> {
  const ctx: ResolveCtx = { tools: {}, mcpTools: {}, memory: {}, skills: {}, agents: {} };
  for (const [name, node] of Object.entries(specNodes)) {
    if (node.type === "tool") ctx.tools[name] = new Tool({ name, description: node.description, run: node.run });
    else if (node.type === "memory") ctx.memory[name] = new Memory(seedOf(node.seed));
    else if (node.type === "skill") ctx.skills[name] = { instructions: node.instructions, uses: node.uses ?? [] };
    else if (node.type === "mcp") ctx.mcpTools[name] = await expandMcp({ prefix: name, server: node.server, env: node.env, tools: node.tools });
  }
  return ctx;
}

function seedOf(seed?: Record<string, string>): Record<string, string> | undefined {
  return seed && Object.keys(seed).length ? seed : undefined;
}

function buildAgentShells(specNodes: Record<string, SpecNode>, agents: Record<string, Agent>): void {
  for (const [name, node] of Object.entries(specNodes)) {
    if (node.type !== "agent") continue;
    agents[name] = new Agent({
      name,
      description: node.description,
      model: node.model,
      system: node.system ?? "",
      temperature: node.temperature,
    });
  }
}

function wireAgents(specNodes: Record<string, SpecNode>, ctx: ResolveCtx): void {
  for (const [name, node] of Object.entries(specNodes)) {
    if (node.type !== "agent") continue;
    const resolved = resolveUses(node.uses ?? [], ctx, name);
    const agent = ctx.agents[name]!;
    agent.tools = resolved.tools;
    agent.memory = resolved.memory;
    agent.system = [resolved.instructions.join("\n\n"), node.system ?? ""].filter(Boolean).join("\n\n");
  }
}

// Flow nodes routable by transitions: agents, fns, nested graphs.
async function buildFlow(
  specNodes: Record<string, SpecNode>,
  agents: Record<string, Agent>,
): Promise<{ flow: Record<string, GraphNode>; graphs: Record<string, Graph> }> {
  const flow: Record<string, GraphNode> = {};
  const graphs: Record<string, Graph> = {};
  for (const [name, node] of Object.entries(specNodes)) {
    if (node.type === "agent") flow[name] = agents[name]!;
    else if (node.type === "fn") flow[name] = node.run;
    else if (node.type === "graph") {
      const sub = await buildScope(node.nodes, node.transitions);
      graphs[name] = sub.graph;
      flow[name] = sub.graph;
    }
  }
  return { flow, graphs };
}

// Turn an agent's `uses` names into concrete tools, injected skill instructions,
// and an attached memory. Dispatch is by what the name resolves to.
function resolveUses(
  names: string[],
  ctx: ResolveCtx,
  owner: string,
): { tools: Tool[]; instructions: string[]; memory?: Memory } {
  const tools: Tool[] = [];
  const instructions: string[] = [];
  let memory: Memory | undefined;

  for (const name of names) {
    if (ctx.tools[name]) tools.push(ctx.tools[name]!);
    else if (ctx.mcpTools[name]) tools.push(...ctx.mcpTools[name]!);
    else if (ctx.skills[name]) {
      instructions.push(ctx.skills[name]!.instructions);
      tools.push(...skillTools(ctx.skills[name]!, ctx));
    } else if (ctx.agents[name]) tools.push(delegationTool(ctx.agents[name]!));
    else if (ctx.memory[name]) memory = ctx.memory[name];
    else throw new Error(`node '${owner}' uses unknown node '${name}'`);
  }
  return { tools, instructions, memory };
}

function skillTools(skill: Skill, ctx: ResolveCtx): Tool[] {
  const out: Tool[] = [];
  for (const t of skill.uses) {
    if (ctx.tools[t]) out.push(ctx.tools[t]!);
    else if (ctx.mcpTools[t]) out.push(...ctx.mcpTools[t]!);
  }
  return out;
}

// Expose an agent to a caller as a callable tool (its own call, own context).
function delegationTool(agent: Agent): Tool {
  return new Tool({
    name: agent.name,
    description: agent.description ?? `Delegate a task to ${agent.name}.`,
    run: (input) => agent.run(input).then((m) => m.content),
  });
}

async function runCommand(spec: SystemSpec, graph: Graph, name: string, input?: string) {
  const cmd = spec.commands?.[name];
  if (!cmd) throw new Error(`no command named '${name}'`);
  const text = input ?? cmd.input ?? "";
  bus.emit({ type: "command", agent: name, input: text });
  return graph.trigger(text, cmd.target);
}

// Build the system, then run the spec's entrypoint.
export async function runSystem(def: SystemDefinition): Promise<System> {
  const sys = await buildSystem(def.spec); // registers hooks first
  await bus.fire("sessionStart");
  if (def.spec.run) await def.spec.run(sys);
  await bus.fire("stop");
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
