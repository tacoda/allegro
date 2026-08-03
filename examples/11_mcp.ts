// An mcp node launches an external MCP server and expands it into callable tools
// an agent may `use`. Declaring `tools` lets the graph build offline (the client
// connects lazily on first call).
//
//   allegro run examples/11_mcp.ts --mock
//   # real run needs the server on PATH and a key:
//   OPENAI_API_KEY=sk-... allegro run examples/11_mcp.ts

import { defineSystem } from "../src/index.ts";
import { mock } from "./_mock.ts";

export default defineSystem({
  nodes: {
    fs: {
      type: "mcp",
      server: "npx -y @modelcontextprotocol/server-filesystem /tmp",
      tools: ["read_file", "list_directory"],
    },
    librarian: { type: "agent", system: "Use the filesystem tools to answer.", uses: ["fs"] },
  },
  transitions: { entry: "librarian", librarian: "end" },
  run: async (sys) => {
    mock(() => "(mock) I'd call list_directory on /tmp.");
    // The server's tools were expanded and wired onto the agent:
    console.log("[tools]", sys.agents.librarian!.tools.map((t) => t.name).join(", "));
    console.log((await sys.run("List the files in /tmp")).content);
  },
});
