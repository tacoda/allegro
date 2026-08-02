import type { RuntimeEvent } from "../otp/index.ts";

// The shared view-model for every UI surface (CLI --events, TUI, web): a process
// table and a one-line event format, both derived from the runtime event stream.

export interface ProcRow {
  pid: number;
  kind: string;
  status: "alive" | "dead";
}

// Fold one event into the process table (returns a new map for React state).
export function applyEvent(procs: Map<number, ProcRow>, e: RuntimeEvent): Map<number, ProcRow> {
  const next = new Map(procs);
  switch (e.type) {
    case "spawn":
      next.set(e.pid, { pid: e.pid, kind: e.kind, status: "alive" });
      break;
    case "exit": {
      const row = next.get(e.pid);
      if (row) next.set(e.pid, { ...row, status: "dead" });
      break;
    }
  }
  return next;
}

export function formatEvent(e: RuntimeEvent): string {
  switch (e.type) {
    case "spawn":
      return `+ spawn #${e.pid} (${e.kind})`;
    case "exit":
      return `- exit  #${e.pid}: ${e.reason}`;
    case "restart":
      return `~ restart #${e.oldPid} -> #${e.newPid}`;
    case "register":
      return `@ ${e.name} = #${e.pid}`;
    case "agent":
      return `${e.phase === "start" ? ">" : "<"} agent ${e.name}: ${e.text.slice(0, 60)}`;
    case "log":
      return `  ${e.message}`;
  }
}
