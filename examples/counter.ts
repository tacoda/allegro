// A GenServer — a stateful process. Callbacks are plain typed methods.
//
//   allegro run examples/counter.ts

import { defineSystem, GenServer } from "../src/index.ts";

class Counter extends GenServer<number> {
  handleCast(_msg: string, state: number) {
    return state + 1;
  }
  handleCall(_msg: string, state: number) {
    return this.reply(state, state);
  }
}

export default defineSystem({
  servers: { Counter },
  run: async () => {
    const c = await Counter.start(0);
    c.cast("inc");
    c.cast("inc");
    c.cast("inc");
    console.log(await c.call("get")); // 3
  },
});
