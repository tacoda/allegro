import { AsyncQueue } from "./queue.ts";

export type Pid = number;

// Events the runtime broadcasts. Every UI surface (TUI, web) subscribes to this
// one stream; the CLI ignores it. One shared view-model, many renderers.
export type RuntimeEvent =
  | { type: "spawn"; pid: Pid; kind: string }
  | { type: "exit"; pid: Pid; reason: string }
  | { type: "restart"; supervisor: Pid; oldPid: Pid; newPid: Pid }
  | { type: "register"; name: string; pid: Pid }
  | { type: "agent"; phase: "start" | "finish"; name: string; text: string }
  | { type: "log"; message: string };

export interface Proc {
  pid: Pid;
  kind: string;
  mailbox: AsyncQueue<any>;
  alive: boolean;
  onExit: ((reason: string) => void)[];
}

// Owns the process table, name registry, and event stream. Scheduling is the
// JavaScript event loop: a process runs until it awaits its mailbox, which
// yields to every other ready process. No run queue, no manual pumping.
export class Runtime {
  private procs = new Map<Pid, Proc>();
  private names = new Map<string, Pid>();
  private nextPid = 1;
  private listeners = new Set<(event: RuntimeEvent) => void>();

  // Wipe all state — used between tests.
  reset(): void {
    this.procs.clear();
    this.names.clear();
    this.listeners.clear();
    this.nextPid = 1;
  }

  subscribe(fn: (event: RuntimeEvent) => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  emit(event: RuntimeEvent): void {
    for (const fn of this.listeners) fn(event);
  }

  // Register a process and run its loop. A loop that returns exits "normal"; one
  // that throws exits with the error as its reason — that is crash isolation:
  // the rejection stays local and monitors are notified.
  spawnProc(kind: string, loop: (proc: Proc) => Promise<void>): Proc {
    const pid = this.nextPid++;
    const proc: Proc = { pid, kind, mailbox: new AsyncQueue(), alive: true, onExit: [] };
    this.procs.set(pid, proc);
    this.emit({ type: "spawn", pid, kind });
    loop(proc).then(
      () => this.exit(pid, "normal"),
      (err) => this.exit(pid, String(err?.message ?? err)),
    );
    return proc;
  }

  get(pid: Pid): Proc | undefined {
    return this.procs.get(pid);
  }

  resolve(target: Pid | string): Proc | undefined {
    const pid = typeof target === "string" ? this.names.get(target) : target;
    return pid === undefined ? undefined : this.procs.get(pid);
  }

  send(target: Pid | string, msg: any): void {
    const proc = this.resolve(target);
    if (proc?.alive) proc.mailbox.push(msg);
  }

  exit(pid: Pid, reason: string): void {
    const proc = this.procs.get(pid);
    if (!proc || !proc.alive) return;
    proc.alive = false;
    this.emit({ type: "exit", pid, reason });
    for (const cb of proc.onExit.splice(0)) cb(reason);
  }

  // Notify `cb` when `pid` dies (immediately if it is already gone).
  monitor(pid: Pid, cb: (reason: string) => void): void {
    const proc = this.procs.get(pid);
    if (!proc || !proc.alive) cb("noproc");
    else proc.onExit.push(cb);
  }

  register(name: string, pid: Pid): void {
    this.names.set(name, pid);
    this.emit({ type: "register", name, pid });
  }

  whereis(name: string): Pid | undefined {
    return this.names.get(name);
  }
}

// The ambient runtime a running system uses. Pinned on globalThis so that if the
// module is loaded twice (e.g. a compiled binary running a spec file that pulls
// in its own copy of the library), both halves still share one runtime and one
// event stream. Tests call `runtime.reset()`.
export const runtime: Runtime = ((globalThis as any).__allegroRuntime ??= new Runtime());

export interface ActorCtx {
  self: Pid;
  send: (target: Pid | string, msg: any) => void;
}

// A bare actor: a function of (state, msg) run per message. `{ __stop: true }`
// ends it. Returns its pid; drive it with `send`.
export function spawn<S>(
  handler: (state: S, msg: any, ctx: ActorCtx) => S | Promise<S>,
  initial: S,
  rt: Runtime = runtime,
): Pid {
  const proc = rt.spawnProc("actor", async (proc) => {
    let state = initial;
    const ctx: ActorCtx = { self: proc.pid, send: (t, m) => rt.send(t, m) };
    while (proc.alive) {
      const msg = await proc.mailbox.next();
      if (msg?.__stop) break;
      state = await handler(state, msg, ctx);
    }
  });
  return proc.pid;
}

export function send(target: Pid | string, msg: any, rt: Runtime = runtime): void {
  rt.send(target, msg);
}

export function register(name: string, pid: Pid, rt: Runtime = runtime): void {
  rt.register(name, pid);
}

export function whereis(name: string, rt: Runtime = runtime): Pid | undefined {
  return rt.whereis(name);
}

export function monitor(pid: Pid, cb: (reason: string) => void, rt: Runtime = runtime): void {
  rt.monitor(pid, cb);
}
