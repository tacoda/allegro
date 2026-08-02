// Supervision — a crashed worker is isolated and restarted with fresh state.
//
//   allegro run examples/supervisor.ts

import { defineSystem, GenServer } from "../src/index.ts";

class Worker extends GenServer<number> {
  handleCall(msg: string, state: number) {
    if (msg === "crash") throw new Error("boom");
    return this.reply(state, state);
  }
}

export default defineSystem({
  servers: { Worker },
  supervisors: {
    sup: { strategy: "one_for_one", children: [{ server: Worker, args: [42] }] },
  },
  run: async (sys) => {
    const w = sys.supervisors.sup!.whichChildren()[0]!;
    console.log("before:   ", await w.call("id")); // 42

    await w.call("crash").catch(() => {}); // kills the worker
    await new Promise((r) => setTimeout(r, 10)); // let the restart run

    const w2 = sys.supervisors.sup!.whichChildren()[0]!;
    console.log("restarted:", w.pid !== w2.pid); // true
    console.log("after:    ", await w2.call("id")); // 42 (fresh)
  },
});
