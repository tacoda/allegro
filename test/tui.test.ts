import { expect, test, beforeEach } from "bun:test";
import React from "react";
import { render } from "ink-testing-library";
import { App } from "../src/tui/app.tsx";
import { runtime } from "../src/otp/index.ts";

beforeEach(() => runtime.reset());

const tick = (ms = 40) => new Promise((r) => setTimeout(r, ms));

test("TUI renders the process table, events, and spec output", async () => {
  const { lastFrame } = render(React.createElement(App, { file: "examples/supervisor.ts" }));
  await tick();
  const frame = lastFrame() ?? "";
  expect(frame).toContain("allegro");
  expect(frame).toContain("processes");
  expect(frame).toContain("events");
  // the supervisor spec restarts a worker and prints its id
  expect(frame).toContain("restart");
  expect(frame).toContain("after:");
});
