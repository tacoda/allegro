import { runtime, Runtime, type Pid } from "./runtime.ts";
import type { ServerRef } from "./genserver.ts";

// How to (re)start one child.
export interface ChildSpec {
  start: () => ServerRef | Promise<ServerRef>;
  id?: string;
}

export interface SupervisorRef {
  pid: Pid;
  whichChildren(): ServerRef[];
}

export interface SupervisorOptions {
  strategy?: "one_for_one";
  maxRestarts?: number;
  children: ChildSpec[];
}

// A child spec from a GenServer class plus its start args.
export function child(
  server: { start: (...args: any[]) => Promise<ServerRef> },
  ...args: any[]
): ChildSpec {
  return { start: () => server.start(...args) };
}

// Starts and monitors children, restarting one that crashes (any exit reason
// but "normal") until the restart budget runs out. one_for_one only, for now:
// each child is restarted independently.
export class Supervisor {
  static async start(spec: SupervisorOptions, rt: Runtime = runtime): Promise<SupervisorRef> {
    const max = spec.maxRestarts ?? 5;
    let restarts = 0;
    const slots: { spec: ChildSpec; ref: ServerRef }[] = [];

    // The supervisor is itself a process, so it has an identity and lifetime.
    const supProc = rt.spawnProc("supervisor", async (proc) => {
      while (proc.alive) {
        const msg = await proc.mailbox.next();
        if (msg?.__stop) break;
      }
    });

    const supervise = async (cs: ChildSpec): Promise<ServerRef> => {
      const ref = await cs.start();
      rt.monitor(ref.pid, (reason) => {
        if (reason === "normal" || restarts >= max) return;
        restarts++;
        void supervise(cs).then((next) => {
          const slot = slots.find((s) => s.spec === cs);
          if (slot) slot.ref = next;
          rt.emit({ type: "restart", supervisor: supProc.pid, oldPid: ref.pid, newPid: next.pid });
        });
      });
      return ref;
    };

    for (const cs of spec.children) {
      slots.push({ spec: cs, ref: await supervise(cs) });
    }

    return { pid: supProc.pid, whichChildren: () => slots.map((s) => s.ref) };
  }
}
