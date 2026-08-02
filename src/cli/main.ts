#!/usr/bin/env bun
import { parseArgs } from "util";
import { runtime } from "../otp/index.ts";
import { loadDefinition, runSystem } from "../spec/index.ts";
import { formatEvent } from "../ui/format.ts";

const USAGE = `allegro — agentic systems on an OTP-style process runtime

usage:
  allegro run <spec.ts|spec.json> [--events]   run a system headless
  allegro tui <spec>                            run with the terminal UI
  allegro web <spec> [--port <n>]               run with the web UI

flags:
  --events   stream runtime events (spawn/exit/restart/agent) to stderr`;

const HELP = new Set([undefined, "help", "-h", "--help"]);

async function main(): Promise<void> {
  const [cmd, ...rest] = process.argv.slice(2);
  if (HELP.has(cmd)) {
    console.log(USAGE);
    return;
  }
  switch (cmd) {
    case "run":
      return cmdRun(rest);
    case "tui": {
      const { runTui } = await import("../tui/main.ts");
      return runTui(specArg(rest));
    }
    case "web": {
      const { runWeb } = await import("../web/main.ts");
      const { positionals, values } = parseArgs({
        args: rest,
        options: { port: { type: "string" } },
        allowPositionals: true,
      });
      return runWeb(positionals[0], values.port ? Number(values.port) : undefined);
    }
    default:
      console.error(`unknown command: ${cmd}\n\n${USAGE}`);
      process.exit(2);
  }
}

async function cmdRun(argv: string[]): Promise<void> {
  const { positionals, values } = parseArgs({
    args: argv,
    options: { events: { type: "boolean" } },
    allowPositionals: true,
  });
  const file = positionals[0];
  if (!file) {
    console.error("usage: allegro run <spec.ts|spec.json>");
    process.exit(2);
  }
  if (values.events) runtime.subscribe((e) => console.error(formatEvent(e)));
  await runSystem(await loadDefinition(file));
}

function specArg(argv: string[]): string {
  const file = argv.find((a) => !a.startsWith("-"));
  if (!file) {
    console.error("usage: allegro <tui|web> <spec>");
    process.exit(2);
  }
  return file;
}

main().catch((err) => {
  console.error(`error: ${err?.message ?? err}`);
  process.exit(1);
});
