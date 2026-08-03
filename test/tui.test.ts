import { expect, test, beforeEach } from "bun:test";
import React from "react";
import { render } from "ink-testing-library";
import { App } from "../src/tui/app.tsx";
import { bus } from "../src/runtime/bus.ts";

beforeEach(() => bus.reset());

const tick = (ms = 60) => new Promise((r) => setTimeout(r, ms));

test("TUI renders the node table, events, and spec output", async () => {
  const { lastFrame } = render(React.createElement(App, { file: "examples/04_pipeline.ts" }));
  await tick();
  const frame = lastFrame() ?? "";
  expect(frame).toContain("allegro");
  expect(frame).toContain("nodes");
  expect(frame).toContain("events");
  // the offline pipeline transforms "  hello  " -> "HELLO!"
  expect(frame).toContain("HELLO!");
});
