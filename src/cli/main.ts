#!/usr/bin/env bun
import { parseArgs } from "util";
import { bus } from "../runtime/bus.ts";
import { loadDefinition, runSystem, buildSystem } from "../spec/index.ts";
import { formatEvent } from "../ui/format.ts";

const USAGE = `allegro — a system is a graph: nodes, transitions, triggers

usage:
  allegro run <spec.ts|spec.json> [--events]        run a system headless
  allegro run <spec> --command <name> [--input <s>] invoke a command
  allegro tui <spec>                                run with the terminal UI
  allegro web <spec> [--port <n>]                   run with the web UI

flags:
  --events   stream lifecycle events (agent/tool/hook/node) to stderr
  --mock     use a canned model backend (no OPENAI_API_KEY needed)`;

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
    options: {
      events: { type: "boolean" },
      mock: { type: "boolean" },
      command: { type: "string" },
      input: { type: "string" },
    },
    allowPositionals: true,
  });
  const file = positionals[0];
  if (!file) {
    console.error("usage: allegro run <spec.ts|spec.json>");
    process.exit(2);
  }
  if (values.mock) process.env.ALLEGRO_MOCK = "1";
  if (values.events) bus.subscribe((e) => console.error(formatEvent(e)));

  const def = await loadDefinition(file);
  if (values.command) {
    const sys = await buildSystem(def.spec);
    const out = await sys.command(values.command, values.input);
    console.log(out.content);
    return;
  }
  await runSystem(def);
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
