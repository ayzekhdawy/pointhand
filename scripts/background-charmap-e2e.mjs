// Background input in a classic Win32 app that ships with Windows: Character Map.
// Usage: node scripts/background-charmap-e2e.mjs <extracted-package-root>
// Opens its own Character Map, pushes it behind the user's window without
// activation, then clicks and types into "Characters to copy" with background:true.
// Closes only the Character Map process it started.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const root = resolve(process.argv[2] ?? ".");
const sdk = resolve(root, "client/node_modules/@modelcontextprotocol/sdk/dist/esm/client");
const { Client } = await import(pathToFileURL(resolve(sdk, "index.js")).href);
const { StdioClientTransport } = await import(pathToFileURL(resolve(sdk, "stdio.js")).href);
const pause = (ms) => new Promise((done) => setTimeout(done, ms));

function foreground() {
  const script =
    "Add-Type -Namespace W -Name U -MemberDefinition '[DllImport(\"user32.dll\")] public static extern System.IntPtr GetForegroundWindow();'; [W.U]::GetForegroundWindow().ToInt64()";
  return Number(execFileSync("powershell.exe", ["-NoProfile", "-Command", script], { encoding: "utf8" }).trim());
}

async function call(client, name, args) {
  const result = await client.callTool({ name, arguments: args });
  const part = result.content.find((item) => item.type === "text");
  if (result.isError) throw new Error(`${name}: ${part?.text}`);
  return JSON.parse(part.text);
}

const running = execFileSync("tasklist.exe", ["/FI", "IMAGENAME eq charmap.exe", "/FO", "CSV", "/NH"], { encoding: "utf8" });
if (/charmap\.exe/i.test(running)) {
  console.log(JSON.stringify({ ok: false, skipped: "another Character Map is open; close it first" }));
  process.exit(2);
}
spawn("charmap.exe", [], { detached: true, stdio: "ignore" }).unref();
const env = { ...process.env, POINTHAND_ALLOW: "charmap.exe", POINTHAND_OVERLAY: "0" };
delete env.POINTHAND_HOST;
delete env.POINTHAND_BACKGROUND;
const client = new Client({ name: "pointhand-background-charmap", version: "1" });
const report = {};
let pid;
try {
  await client.connect(
    new StdioClientTransport({ command: process.execPath, args: [resolve(root, "client/dist/index.js")], cwd: root, env }),
  );
  let window;
  for (let attempt = 0; attempt < 30 && !window; attempt++) {
    window = (await call(client, "list_windows", {})).find((row) => /charmap\.exe$/i.test(row.app ?? ""));
    if (!window) await pause(500);
  }
  assert.ok(window, "Character Map did not appear");
  pid = window.pid;
  await pause(1000);
  // A normal minimize hands activation to the next window, so the target ends
  // up behind the user's work. If the user is busy, fall back to background.
  await call(client, "set_window_state", { window_id: window.id, state: "minimize" }).catch(() =>
    call(client, "set_window_state", { window_id: window.id, state: "minimize", background: true }),
  );
  await call(client, "set_window_state", { window_id: window.id, state: "restore", background: true });
  await pause(500);
  report.target_behind_at_start = foreground() !== window.id;

  const field = (await call(client, "find_elements", { window_id: window.id, role: "Edit", limit: 10 })).matches[0];
  assert.ok(field, "no edit field");
  report.field = field.line;
  report.click = await call(client, "click", { element_index: field.element_index, background: true });
  await call(client, "type_text", { window_id: window.id, text: "PointHand", background: true });
  await call(client, "press_key", { window_id: window.id, key: "End", background: true });
  await pause(400);
  report.value = (await call(client, "get_element", { element_index: field.element_index })).value;
  report.chord_refused = await call(client, "press_key", { window_id: window.id, key: "ctrl+a", background: true })
    .then(() => false)
    .catch((error) => /modifier/.test(String(error)));
  report.target_stayed_behind = foreground() !== window.id;

  assert.ok(String(report.value).startsWith("PointHand"), "typed text did not reach the field");
  assert.ok(report.chord_refused, "a modifier chord was not refused in background mode");
  assert.ok(report.target_stayed_behind, "background input brought Character Map forward");
  report.ok = true;
} catch (error) {
  report.ok = false;
  report.error = error instanceof Error ? error.message : String(error);
  process.exitCode = 1;
} finally {
  await client.close().catch(() => {});
  if (pid) {
    try {
      execFileSync("taskkill.exe", ["/PID", String(pid), "/F"], { stdio: "ignore" });
    } catch {}
  }
  console.log(JSON.stringify(report, null, 2));
}
