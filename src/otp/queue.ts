// An async FIFO queue: a process's mailbox. `next()` resolves immediately if a
// message is buffered, otherwise waits until one is pushed. This is the only
// primitive the actor loop blocks on, so awaiting it is how a process yields.
export class AsyncQueue<T> {
  private buffer: T[] = [];
  private waiters: ((value: T) => void)[] = [];

  push(value: T): void {
    const waiter = this.waiters.shift();
    if (waiter) waiter(value);
    else this.buffer.push(value);
  }

  next(): Promise<T> {
    if (this.buffer.length > 0) return Promise.resolve(this.buffer.shift()!);
    return new Promise<T>((resolve) => this.waiters.push(resolve));
  }

  get size(): number {
    return this.buffer.length;
  }
}
