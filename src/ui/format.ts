import type { BusEvent } from "../runtime/bus.ts";

// The shared view-model for every UI surface (CLI --events, TUI, web): a running
// list of active nodes and a one-line event format, both derived from the bus.

export interface NodeRow {
  name: string;
  kind: string;
  status: "running" | "done";
}

// Fold one event into the node table (returns a new map for React state).
export function applyEvent(rows: Map<string, NodeRow>, e: BusEvent): Map<string, NodeRow> {
  const next = new Map(rows);
  const name = e.agent ?? e.tool ?? "";
  switch (e.type) {
    case "nodeEnter":
    case "agentStart":
      if (name) next.set(name, { name, kind: e.type === "agentStart" ? "agent" : "node", status: "running" });
      break;
    case "nodeExit":
    case "agentFinish": {
      const row = next.get(name);
      if (row) next.set(name, { ...row, status: "done" });
      break;
    }
  }
  return next;
}

// One formatter per event type — a dispatch table, not a switch.
const FORMATTERS: Partial<Record<BusEvent["type"], (e: BusEvent, who: string) => string>> = {
  sessionStart: () => "· session start",
  userPromptSubmit: (e) => `> prompt: ${(e.text ?? "").slice(0, 60)}`,
  command: (e, who) => `$ command ${who}: ${(e.input ?? "").slice(0, 50)}`,
  nodeEnter: (_e, who) => `→ ${who}`,
  nodeExit: (_e, who) => `← ${who}`,
  agentStart: (e, who) => `> agent ${who}: ${(e.input ?? "").slice(0, 50)}`,
  agentFinish: (e, who) => `< agent ${who}: ${(e.output ?? "").slice(0, 50)}`,
  preToolUse: (e, who) => `⚙ tool ${who}(${(e.input ?? "").slice(0, 30)})`,
  postToolUse: (e, who) => `✓ tool ${who} → ${(e.output ?? "").slice(0, 30)}`,
  stop: () => "· stop",
  log: (e) => `  ${e.message ?? ""}`,
};

export function formatEvent(e: BusEvent): string {
  const who = e.agent ?? e.tool ?? "";
  return FORMATTERS[e.type]?.(e, who) ?? `  ${e.type}`;
}
