import { runtime, Runtime, type Pid } from "./runtime.ts";

// A GenServer's handle_call return: a value for the caller plus the next state.
export interface Reply<S> {
  value: any;
  state: S;
}

// A handle to a running server. `call` awaits a reply; `cast` is fire-and-forget.
export interface ServerRef<Call = any, Cast = any> {
  pid: Pid;
  call(msg: Call): Promise<any>;
  cast(msg: Cast): void;
  stop(): void;
  alive(): boolean;
}

type Envelope =
  | { kind: "call"; payload: any; resolve: (v: any) => void; reject: (e: any) => void }
  | { kind: "cast"; payload: any }
  | { kind: "stop" };

// A stateful server process with a class shape. Subclass and override the
// callbacks; `init` returns the initial state, `handleCast` returns the next
// state, `handleCall` returns `reply(value, state)`. Start with `.start(...)`.
export abstract class GenServer<S = any, Call = any, Cast = any> {
  init(...args: any[]): S | Promise<S> {
    return args[0] as S;
  }

  handleCast(_msg: Cast, state: S): S | Promise<S> {
    return state;
  }

  handleCall(_msg: Call, state: S): Reply<S> | Promise<Reply<S>> {
    return this.reply(undefined, state);
  }

  protected reply(value: any, state: S): Reply<S> {
    return { value, state };
  }

  static async start<T extends GenServer>(
    this: new () => T,
    ...args: any[]
  ): Promise<ServerRef> {
    return startServer(new this(), args, runtime);
  }
}

export async function startServer(
  inst: GenServer,
  args: any[],
  rt: Runtime = runtime,
): Promise<ServerRef> {
  let state = await inst.init(...args);
  const proc = rt.spawnProc("genserver", async (proc) => {
    while (proc.alive) {
      const env: Envelope = await proc.mailbox.next();
      if (env.kind === "stop") break;
      if (env.kind === "cast") {
        state = await inst.handleCast(env.payload, state);
      } else {
        try {
          const r = await inst.handleCall(env.payload, state);
          state = r.state;
          env.resolve(r.value);
        } catch (err) {
          env.reject(err); // fail the caller, then crash so a supervisor restarts
          throw err;
        }
      }
    }
  });
  return makeRef(proc.pid, rt);
}

function makeRef(pid: Pid, rt: Runtime): ServerRef {
  return {
    pid,
    call: (msg) =>
      new Promise((resolve, reject) => {
        const proc = rt.get(pid);
        if (!proc?.alive) return reject(new Error("noproc"));
        proc.mailbox.push({ kind: "call", payload: msg, resolve, reject });
      }),
    cast: (msg) => rt.send(pid, { kind: "cast", payload: msg }),
    stop: () => rt.send(pid, { kind: "stop" }),
    alive: () => !!rt.get(pid)?.alive,
  };
}
