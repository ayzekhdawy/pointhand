// Background drawing in a real app: Microsoft Paint.
// Usage: node scripts/background-paint-e2e.mjs <extracted-package-root>
// Opens its own Paint window, draws a rectangle with background:true, and
// compares window captures before and after. Refuses to run while another
// Paint is open, and closes only the Paint process it started.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const root = resolve(process.argv[2] ?? ".");
const sdk = resolve(root, "client/node_modules/@modelcontextprotocol/sdk/dist/esm/client");
const { Client } = await import(pathToFileURL(resolve(sdk, "index.js")).href);
const { StdioClientTransport } = await import(pathToFileURL(resolve(sdk, "stdio.js")).href);

const pause = (ms) => new Promise((done) => setTimeout(done, ms));

function desktopState() {
  const script =
    "Add-Type -Namespace W -Name U -MemberDefinition '[DllImport(\"user32.dll\")] public static extern System.IntPtr GetForegroundWindow(); [DllImport(\"user32.dll\")] public static extern bool GetCursorPos(out System.Drawing.Point p);' -ReferencedAssemblies System.Drawing; " +
    "$p = New-Object System.Drawing.Point; [void][W.U]::GetCursorPos([ref]$p); " +
    "'{0} {1} {2}' -f [W.U]::GetForegroundWindow().ToInt64(), $p.X, $p.Y";
  const [foreground, x, y] = execFileSync("powershell.exe", ["-NoProfile", "-Command", script], { encoding: "utf8" })
    .trim()
    .split(" ")
    .map(Number);
  return { foreground, x, y };
}

async function callRaw(client, name, args) {
  const result = await client.callTool({ name, arguments: args });
  const part = result.content.find((item) => item.type === "text");
  if (result.isError) throw new Error(`${name}: ${part?.text}`);
  return { json: JSON.parse(part.text), images: result.content.filter((item) => item.type === "image") };
}

const paintRunning = execFileSync("tasklist.exe", ["/FI", "IMAGENAME eq mspaint.exe", "/FO", "CSV", "/NH"], { encoding: "utf8" });
if (/mspaint\.exe/i.test(paintRunning)) {
  console.log(JSON.stringify({ ok: false, skipped: "another Paint is open; close it first" }));
  process.exit(2);
}

spawn("mspaint.exe", [], { detached: true, stdio: "ignore" }).unref();
const env = { ...process.env, POINTHAND_ALLOW: "mspaint.exe", POINTHAND_OVERLAY: "0" };
delete env.POINTHAND_HOST;
delete env.POINTHAND_BACKGROUND;
const client = new Client({ name: "pointhand-background-paint", version: "1" });
const report = {};
let paintPid;
try {
  await client.connect(
    new StdioClientTransport({ command: process.execPath, args: [resolve(root, "client/dist/index.js")], cwd: root, env }),
  );
  let window;
  for (let attempt = 0; attempt < 40 && !window; attempt++) {
    const rows = (await callRaw(client, "list_windows", {})).json;
    window = rows.find((row) => /mspaint\.exe$/i.test(row.app ?? ""));
    if (!window) await pause(500);
  }
  assert.ok(window, "Paint window did not appear");
  paintPid = window.pid;
  await pause(2500);
  // A normal minimize hands activation to the next window, so the target ends
  // up behind the user's work. If the user is busy, fall back to background.
  await callRaw(client, "set_window_state", { window_id: window.id, state: "minimize" }).catch(() =>
    callRaw(client, "set_window_state", { window_id: window.id, state: "minimize", background: true }),
  );
  await callRaw(client, "set_window_state", { window_id: window.id, state: "restore", background: true });
  await pause(500);
  const start = desktopState();
  report.target_behind_at_start = start.foreground !== window.id;

  // The drawing surface is the largest pane; find it through the tree.
  const panes = (await callRaw(client, "find_elements", { window_id: window.id, role: "Pane", limit: 200 })).json.matches;
  let canvas = null;
  let best = 0;
  for (const pane of panes) {
    const info = (await callRaw(client, "get_element", { element_index: pane.element_index })).json;
    if (!info.rect) continue;
    const [l, t, r, b] = info.rect;
    const area = (r - l) * (b - t);
    if (area > best && t > window.rect[1] + 120) {
      best = area;
      canvas = info.rect;
    }
  }
  assert.ok(canvas, "canvas not found");
  report.canvas = canvas;

  const shot = async () => {
    const result = await callRaw(client, "get_window_state", { window_id: window.id, include_screenshot: true, background: true, scale: 1 });
    return result.images[0]?.data ?? "";
  };
  const before = await shot();

  // A rectangle in window pixels, well inside the canvas.
  const left = canvas[0] - window.rect[0] + 40;
  const top = canvas[1] - window.rect[1] + 40;
  const points = [
    [left, top],
    [left + 160, top],
    [left + 160, top + 100],
    [left, top + 100],
    [left, top],
  ];
  report.draw = (await callRaw(client, "draw", { window_id: window.id, points, background: true })).json;
  await pause(800);
  const after = await shot();
  report.capture_changed = before.length > 0 && after.length > 0 && before !== after;

  const end = desktopState();
  report.target_stayed_behind = end.foreground !== window.id;
  report.pointer_moved_by_anyone = !(end.x === start.x && end.y === start.y);
  assert.ok(report.capture_changed, "the canvas did not change; Paint ignored the posted stroke");
  assert.ok(report.target_stayed_behind, "background drawing brought Paint forward");
  report.ok = true;
} catch (error) {
  report.ok = false;
  report.error = error instanceof Error ? error.message : String(error);
  process.exitCode = 1;
} finally {
  await client.close().catch(() => {});
  if (paintPid) {
    try {
      execFileSync("taskkill.exe", ["/PID", String(paintPid), "/F"], { stdio: "ignore" });
    } catch {}
  }
  console.log(JSON.stringify(report, null, 2));
}
