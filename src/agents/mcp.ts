import { Tool } from "./tool.ts";

// A minimal MCP stdio client: launches the server, speaks newline-delimited
// JSON-RPC, and exposes tools/list + tools/call. Connection is lazy — the first
// call spawns the process; declared tool names let the build stay offline.
export class McpClient {
  private proc?: any;
  private buf = "";
  private pending = new Map<number, { resolve: (v: any) => void; reject: (e: any) => void }>();
  private nextId = 1;

  constructor(
    private command: string,
    private env?: Record<string, string>,
  ) {}

  private async connect(): Promise<void> {
    if (this.proc) return;
    const [cmd, ...args] = this.command.split(" ");
    this.proc = Bun.spawn([cmd!, ...args], {
      stdin: "pipe",
      stdout: "pipe",
      stderr: "inherit",
      env: { ...process.env, ...this.env },
    });
    this.readLoop();
    await this.rpc("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "allegro", version: "0.1.0" },
    });
    this.notify("notifications/initialized");
  }

  private async readLoop(): Promise<void> {
    const reader = this.proc.stdout.getReader();
    const decoder = new TextDecoder();
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      this.buf += decoder.decode(value, { stream: true });
      this.drainLines();
    }
  }

  // Dispatch every complete newline-delimited JSON-RPC message in the buffer.
  private drainLines(): void {
    let nl: number;
    while ((nl = this.buf.indexOf("\n")) >= 0) {
      const line = this.buf.slice(0, nl).trim();
      this.buf = this.buf.slice(nl + 1);
      if (line) this.onMessage(JSON.parse(line));
    }
  }

  private onMessage(msg: any): void {
    if (msg.id == null) return;
    const p = this.pending.get(msg.id);
    if (!p) return;
    this.pending.delete(msg.id);
    if (msg.error) p.reject(new Error(msg.error.message ?? "mcp error"));
    else p.resolve(msg.result);
  }

  private write(obj: any): void {
    this.proc.stdin.write(JSON.stringify(obj) + "\n");
  }

  private notify(method: string, params?: any): void {
    this.write({ jsonrpc: "2.0", method, params });
  }

  private async rpc(method: string, params?: any): Promise<any> {
    await (method === "initialize" ? Promise.resolve() : this.connect());
    const id = this.nextId++;
    const done = new Promise<any>((resolve, reject) => this.pending.set(id, { resolve, reject }));
    this.write({ jsonrpc: "2.0", id, method, params });
    return done;
  }

  async listTools(): Promise<{ name: string; description?: string }[]> {
    const res = await this.rpc("tools/list");
    return res.tools ?? [];
  }

  async callTool(name: string, input: string): Promise<string> {
    const res = await this.rpc("tools/call", { name, arguments: { input } });
    const parts = (res.content ?? []).map((c: any) => c.text ?? "").filter(Boolean);
    return parts.join("\n") || JSON.stringify(res);
  }

  close(): void {
    this.proc?.kill?.();
  }
}

export interface McpSpec {
  prefix: string;
  server: string;
  env?: Record<string, string>;
  tools?: string[]; // allowlist; omit to enumerate from the server
}

// Expand an mcp node into callable tools. If `tools` is given, stubs are made
// without connecting (offline build); otherwise the server is queried to
// enumerate. Each tool connects lazily on first call.
export async function expandMcp(spec: McpSpec): Promise<Tool[]> {
  const client = new McpClient(spec.server, spec.env);
  const names = spec.tools ?? (await client.listTools()).map((t) => t.name);
  return names.map(
    (name) =>
      new Tool({
        name: `${spec.prefix}_${name}`,
        description: `MCP tool ${name} from ${spec.prefix}`,
        run: (input) => client.callTool(name, input),
      }),
  );
}
