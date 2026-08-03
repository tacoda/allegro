import React, { useEffect, useState } from "react";
import { Box, Text, useApp, useInput, useStdin } from "ink";
import { bus } from "../runtime/bus.ts";
import { loadDefinition, runSystem } from "../spec/index.ts";
import { applyEvent, formatEvent, type NodeRow } from "../ui/format.ts";

type Status = "running" | "done" | "error";

export function App({ file }: { file: string }) {
  const { exit } = useApp();
  const { isRawModeSupported } = useStdin();
  const [procs, setProcs] = useState<Map<string, NodeRow>>(new Map());
  const [log, setLog] = useState<string[]>([]);
  const [output, setOutput] = useState<string[]>([]);
  const [status, setStatus] = useState<Status>("running");
  const [error, setError] = useState("");

  useInput((input) => input === "q" && exit(), { isActive: isRawModeSupported });

  useEffect(() => {
    const unsub = bus.subscribe((e) => {
      setProcs((prev) => applyEvent(prev, e));
      setLog((prev) => [...prev, formatEvent(e)].slice(-12));
    });
    // Route the spec's console output into a panel so it doesn't corrupt the UI.
    const realLog = console.log;
    console.log = (...args: any[]) => setOutput((prev) => [...prev, args.join(" ")].slice(-12));

    (async () => {
      try {
        await runSystem(await loadDefinition(file));
        setStatus("done");
      } catch (err: any) {
        setError(String(err?.message ?? err));
        setStatus("error");
      }
    })();

    return () => {
      unsub();
      console.log = realLog;
    };
  }, [file]);

  return (
    <Box flexDirection="column" paddingX={1}>
      <Text bold color="green">
        allegro · {file}
      </Text>
      <Box marginTop={1}>
        <Nodes rows={procs} />
        <Panel title="events" lines={log} flexGrow={1} />
      </Box>
      <Panel title="output" lines={output} color="yellow" marginTop={1} />
      <StatusLine status={status} error={error} />
    </Box>
  );
}

function Nodes({ rows }: { rows: Map<string, NodeRow> }) {
  const list = [...rows.values()];
  return (
    <Box flexDirection="column" marginRight={3} minWidth={22}>
      <Text bold underline>
        nodes
      </Text>
      {list.map((n) => (
        <Text key={n.name} color={n.status === "running" ? "cyan" : "gray"}>
          {n.status === "running" ? "●" : "✓"} {n.name} {n.kind}
        </Text>
      ))}
      {list.length === 0 && <Text color="gray">(none)</Text>}
    </Box>
  );
}

function Panel({
  title,
  lines,
  color,
  flexGrow,
  marginTop,
}: {
  title: string;
  lines: string[];
  color?: string;
  flexGrow?: number;
  marginTop?: number;
}) {
  if (lines.length === 0 && title === "output") return null;
  return (
    <Box flexDirection="column" flexGrow={flexGrow} marginTop={marginTop}>
      <Text bold underline>
        {title}
      </Text>
      {lines.map((line, i) => (
        <Text key={i} color={color}>
          {line}
        </Text>
      ))}
    </Box>
  );
}

function StatusLine({ status, error }: { status: Status; error: string }) {
  const map = {
    running: <Text color="gray">running…</Text>,
    done: <Text color="green">done — press q to quit</Text>,
    error: <Text color="red">error: {error}</Text>,
  };
  return <Box marginTop={1}>{map[status]}</Box>;
}
