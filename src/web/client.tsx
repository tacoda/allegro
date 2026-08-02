import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import type { RuntimeEvent } from "../otp/runtime.ts";
import { applyEvent, formatEvent, type ProcRow } from "../ui/format.ts";

declare const SPEC_FILE: string;

function App() {
  const [procs, setProcs] = useState<Map<number, ProcRow>>(new Map());
  const [log, setLog] = useState<string[]>([]);
  const [output, setOutput] = useState<string[]>([]);
  const [connected, setConnected] = useState(false);

  useEffect(() => {
    const ws = new WebSocket(`ws://${location.host}/ws`);
    ws.onopen = () => setConnected(true);
    ws.onclose = () => setConnected(false);
    ws.onmessage = (ev) => {
      const e: RuntimeEvent = JSON.parse(ev.data);
      setProcs((prev) => applyEvent(prev, e));
      setLog((prev) => [...prev, formatEvent(e)].slice(-200));
      if (e.type === "log") setOutput((prev) => [...prev, e.message].slice(-200));
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
          <h2>processes</h2>
          {[...procs.values()].map((p) => (
            <div key={p.pid} className={`proc ${p.status}`}>
              <span className="glyph">{p.status === "alive" ? "●" : "✗"}</span> #{p.pid} {p.kind}
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
