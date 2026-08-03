import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import type { BusEvent } from "../runtime/bus.ts";
import { applyEvent, formatEvent, type NodeRow } from "../ui/format.ts";

declare const SPEC_FILE: string;

function App() {
  const [procs, setProcs] = useState<Map<string, NodeRow>>(new Map());
  const [log, setLog] = useState<string[]>([]);
  const [output, setOutput] = useState<string[]>([]);
  const [connected, setConnected] = useState(false);

  useEffect(() => {
    const ws = new WebSocket(`ws://${location.host}/ws`);
    ws.onopen = () => setConnected(true);
    ws.onclose = () => setConnected(false);
    ws.onmessage = (ev) => {
      const e: BusEvent = JSON.parse(ev.data);
      setProcs((prev) => applyEvent(prev, e));
      setLog((prev) => [...prev, formatEvent(e)].slice(-200));
      if (e.type === "log") setOutput((prev) => [...prev, e.message ?? ""].slice(-200));
    };
    return () => ws.close();
  }, []);

  return (
    <div className="wrap">
      <header>
        <h1>allegro</h1>
        <span className="file">{SPEC_FILE}</span>
        <span className={connected ? "dot on" : "dot"}>{connected ? "live" : "offline"}</span>
      </header>
      <div className="cols">
        <section className="procs">
          <h2>nodes</h2>
          {[...procs.values()].map((n) => (
            <div key={n.name} className={`proc ${n.status}`}>
              <span className="glyph">{n.status === "running" ? "●" : "✓"}</span> {n.name} {n.kind}
            </div>
          ))}
          {procs.size === 0 && <div className="muted">(none)</div>}
        </section>
        <section className="events">
          <h2>events</h2>
          <pre>{log.join("\n")}</pre>
        </section>
        <section className="output">
          <h2>output</h2>
          <pre>{output.join("\n") || <span className="muted">(none)</span>}</pre>
        </section>
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
