import { bus, type BusEvent } from "../runtime/bus.ts";
import { loadDefinition, runSystem } from "../spec/index.ts";

// Serve a React UI and stream the runtime's event feed to it over WebSocket. The
// spec runs once; events are buffered and replayed to each client that connects,
// so opening the page after a run still shows the full picture.
export async function runWeb(file?: string, port = 4173): Promise<void> {
  if (!file) {
    console.error("usage: allegro web <spec>");
    process.exit(2);
  }
  const server = await createServer(file, port);
  console.error(`allegro web · http://localhost:${server.port}  (spec: ${file})`);
  try {
    await runSystem(await loadDefinition(file));
  } catch (err: any) {
    bus.emit({ type: "log", message: `error: ${err?.message ?? err}` });
  }
}

// Serve the UI and bridge the runtime event feed to WebSocket clients. Buffers
// events so a client that connects mid- or post-run still sees the whole story.
export async function createServer(file: string, port: number) {
  const clientJs = await bundleClient(file);
  const history: BusEvent[] = [];
  const sockets = new Set<any>();

  bus.subscribe((e) => {
    history.push(e);
    const msg = JSON.stringify(e);
    for (const ws of sockets) ws.send(msg);
  });

  // Route the spec's console output onto the same event feed.
  console.log = (...args: any[]) => bus.emit({ type: "log", message: args.join(" ") });

  return Bun.serve({
    port,
    fetch(req, server) {
      const url = new URL(req.url);
      if (url.pathname === "/ws") {
        return server.upgrade(req) ? undefined : new Response("upgrade failed", { status: 400 });
      }
      if (url.pathname === "/client.js") {
        return new Response(clientJs, { headers: { "content-type": "text/javascript" } });
      }
      return new Response(page(file), { headers: { "content-type": "text/html" } });
    },
    websocket: {
      open(ws) {
        sockets.add(ws);
        for (const e of history) ws.send(JSON.stringify(e));
      },
      close(ws) {
        sockets.delete(ws);
      },
      message() {},
    },
  });
}

async function bundleClient(file: string): Promise<string> {
  const built = await Bun.build({
    entrypoints: [`${import.meta.dir}/client.tsx`],
    target: "browser",
    minify: true,
    define: { SPEC_FILE: JSON.stringify(file) },
  });
  if (!built.success) throw new AggregateError(built.logs, "client bundle failed");
  return built.outputs[0]!.text();
}

function page(file: string): string {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>allegro · ${file}</title>
<style>
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  body { margin: 0; font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; background: #0d1117; color: #c9d1d9; }
  header { display: flex; align-items: center; gap: 12px; padding: 12px 20px; border-bottom: 1px solid #21262d; }
  h1 { margin: 0; font-size: 16px; color: #3fb950; }
  .file { color: #8b949e; }
  .dot { margin-left: auto; padding: 2px 8px; border-radius: 10px; background: #21262d; color: #8b949e; font-size: 12px; }
  .dot.on { background: #12321c; color: #3fb950; }
  .cols { display: grid; grid-template-columns: 220px 1fr 1fr; gap: 1px; background: #21262d; height: calc(100vh - 50px); }
  section { background: #0d1117; padding: 12px 16px; overflow: auto; }
  h2 { margin: 0 0 8px; font-size: 12px; text-transform: uppercase; letter-spacing: .05em; color: #8b949e; }
  .proc { padding: 2px 0; }
  .proc.dead { color: #6e7681; }
  .proc .glyph { color: #58a6ff; }
  .proc.dead .glyph { color: #f85149; }
  pre { margin: 0; white-space: pre-wrap; word-break: break-word; }
  .muted { color: #6e7681; }
  .output pre { color: #d29922; }
</style>
</head>
<body>
<div id="root"></div>
<script type="module" src="/client.js"></script>
</body>
</html>`;
}
