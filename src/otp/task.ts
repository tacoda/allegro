// Green-thread fan-out. On the event loop these are just promises: `async`
// starts work, `await` joins it, `parallel` runs many and joins in input order.
export interface TaskRef<T> {
  promise: Promise<T>;
}

export const Task = {
  async<T>(fn: () => T | Promise<T>): TaskRef<T> {
    return { promise: Promise.resolve().then(fn) };
  },

  await<T>(task: TaskRef<T>): Promise<T> {
    return task.promise;
  },

  parallel<T>(fns: Array<() => T | Promise<T>>): Promise<T[]> {
    return Promise.all(fns.map((fn) => Promise.resolve().then(fn)));
  },
};
