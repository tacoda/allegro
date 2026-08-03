import { expect, test, beforeEach } from "bun:test";
import { createServer } from "../src/web/main.ts";
import { runSystem, loadDefinition } from "../src/spec/index.ts";
import { bus } from "../src/runtime/bus.ts";

beforeEach(() => bus.reset());

test("web server serves the page + client bundle and streams events", async () => {
  const server = await createServer("examples/pipeline.ts", 0);
  const base = `http://localhost:${server.port}`;

  const html = await (await fetch(`${base}/`)).text();
  expect(html).toContain("<title>allegro");
  expect(html).toContain(`id="root"`);
  expect(html).toContain("/client.js");

  const js = await (await fetch(`${base}/client.js`)).text();
  expect(js).toContain("createRoot");
  expect(js.length).toBeGreaterThan(1000);

  // run the spec; its console output flows onto the event feed as "log" events
  const events: any[] = [];
  bus.subscribe((e) => events.push(e));
  await runSystem(await loadDefinition("examples/pipeline.ts"));
  expect(events.some((e) => e.type === "log" && e.message === "big")).toBe(true);

  server.stop(true);
});
