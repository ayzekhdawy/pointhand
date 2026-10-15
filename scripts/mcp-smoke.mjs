// End-to-end smoke test: talk to the PointHand MCP server over stdio like a real client.
// Usage: node scripts/mcp-smoke.mjs <step> [json-args]
//   steps: tools | windows <query> | state <window_id> | call <tool> <json>
import { Client } from "../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js";
import { StdioClientTransport } from "../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = resolve(root, ".scratch");
mkdirSync(outDir, { recursive: true });

const transport = new StdioClientTransport({
  command: process.execPath,
  args: [resolve(root, "client/dist/index.js")],
  env: { ...process.env, POINTHAND_OVERLAY: "0" },
});
const client = new Client({ name: "pointhand-smoke", version: "0.0.1" });
await client.connect(transport);

function show(result, label) {
  for (const [i, part] of result.content.entries()) {
    if (part.type === "text") {
      console.log(part.text);
    } else if (part.type === "image") {
      const file = resolve(outDir, `${label}-${i}.png`);
      writeFileSync(file, Buffer.from(part.data, "base64"));
      console.log(`[image saved: ${file}]`);
    }
  }
  if (result.isError) console.log("[tool reported isError]");
}

const [step, ...rest] = process.argv.slice(2);
try {
  if (step === "tools") {
    const { tools } = await client.listTools();
    console.log(tools.map((t) => t.name).join("\n"));
    console.log(`(${tools.length} tools)`);
  } else if (step === "call") {
    const [tool, json] = rest;
    const started = performance.now();
    const result = await client.callTool({ name: tool, arguments: json ? JSON.parse(json) : {} });
    show(result, tool);
    console.log(`[${tool}: ${Math.round(performance.now() - started)} ms]`);
  } else {
    console.error("unknown step");
    process.exitCode = 2;
  }
} finally {
  await client.close();
}
