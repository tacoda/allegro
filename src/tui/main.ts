import React from "react";
import { render } from "ink";
import { App } from "./app.tsx";

export async function runTui(file: string): Promise<void> {
  const app = render(React.createElement(App, { file }));
  await app.waitUntilExit();
  process.exit(0); // stop any lingering server processes still awaiting mail
}
