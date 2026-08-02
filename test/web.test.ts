import { expect, test, beforeEach } from "bun:test";
import { createServer } from "../src/web/main.ts";
import { runSystem, loadDefinition } from "../src/spec/index.ts";
import { runtime } from "../src/otp/index.ts";

beforeEach(() => runtime.reset());

test("web server serves the page + client bundle and streams events", async () => {
  const server = await createServer("examples/counter.ts", 0);
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
  runtime.subscribe((e) => events.push(e));
  await runSystem(await loadDefinition("examples/counter.ts"));
  expect(events.some((e) => e.type === "log" && e.message === "3")).toBe(true);

  server.stop(true);
});
