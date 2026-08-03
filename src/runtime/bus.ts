import type { Hook, HookEvent, HookPayload, HookResult } from "../spec/define.ts";

// Lifecycle events, broadcast to UI observers and to gating hooks. This is what
// remained after OTP: no scheduler, no processes — just the event stream that
// hooks fire on and every UI surface (CLI --events, TUI, web) subscribes to.
export interface BusEvent extends Partial<HookPayload> {
  type: HookEvent | "command" | "nodeEnter" | "nodeExit" | "log";
  message?: string;
}

type Observer = (e: BusEvent) => void;

// One bus per process (pinned on globalThis so a doubly-loaded module still
// shares one stream — a spec file may pull in its own copy of the library).
export class Bus {
  private observers = new Set<Observer>();
  private hooks: Partial<Record<HookEvent, Hook[]>> = {};

  // UI subscription — fire-and-forget, no decision returned.
  subscribe(fn: Observer): () => void {
    this.observers.add(fn);
    return () => this.observers.delete(fn);
  }

  register(event: HookEvent, hook: Hook | Hook[]): void {
    (this.hooks[event] ??= []).push(...(Array.isArray(hook) ? hook : [hook]));
  }

  // Notify observers, then run matching hooks in order. The first hook that
  // returns a decision (block/replace) wins and short-circuits.
  async fire(event: HookEvent, payload: Omit<HookPayload, "event"> = {}): Promise<HookResult> {
    const ev: HookPayload = { event, ...payload };
    for (const fn of this.observers) fn({ ...ev, type: event });
    for (const h of this.hooks[event] ?? []) {
      if (h.match && !hookMatches(h.match, ev)) continue;
      const result = await h.run(ev);
      if (result) return result;
    }
  }

  // Emit a non-hook event to observers only (nodeEnter/exit, command, log).
  emit(e: BusEvent): void {
    for (const fn of this.observers) fn(e);
  }

  // Drop registered hooks but keep UI observers — a rebuild re-registers hooks
  // without severing a --events subscription made before the run.
  clearHooks(): void {
    this.hooks = {};
  }

  reset(): void {
    this.observers.clear();
    this.hooks = {};
  }
}

function hookMatches(match: string, ev: HookPayload): boolean {
  const target = ev.tool ?? ev.agent ?? "";
  return target.includes(match);
}

export const bus: Bus = ((globalThis as any).__allegroBus ??= new Bus());
