// A persistent key/value store. Attached to an agent, the model gets built-in
// remember/recall tools; `recall` falls back to a fuzzy substring match so a
// later turn can phrase a key differently.
export class Memory {
  private store = new Map<string, string>();

  constructor(seed?: Record<string, string>) {
    if (seed) for (const [k, v] of Object.entries(seed)) this.store.set(k, v);
  }

  remember(key: string, value: string): string {
    this.store.set(key, value);
    return value;
  }

  recall(key: string): string | null {
    const exact = this.store.get(key);
    if (exact !== undefined) return exact;
    const needle = key.toLowerCase();
    for (const [k, v] of this.store) {
      if (k.toLowerCase().includes(needle) || needle.includes(k.toLowerCase())) return v;
    }
    return null;
  }

  forget(key: string): boolean {
    return this.store.delete(key);
  }

  has(key: string): boolean {
    return this.store.has(key);
  }

  keys(): string[] {
    return [...this.store.keys()];
  }

  get size(): number {
    return this.store.size;
  }
}
