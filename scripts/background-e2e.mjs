// Background-mode check while the user keeps working in another window.
// Usage: node scripts/background-e2e.mjs <extracted-package-root>
// Opens its own small WinForms test window (text box, check box, counter
// button), drives it with background:true, and verifies that the user's
// foreground window and pointer did not move after the window opened.
// Closes only the test window's own process.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const root = resolve(process.argv[2] ?? ".");
const sdk = resolve(root, "client/node_modules/@modelcontextprotocol/sdk/dist/esm/client");
const { Client } = await import(pathToFileURL(resolve(sdk, "index.js")).href);
const { StdioClientTransport } = await import(pathToFileURL(resolve(sdk, "stdio.js")).href);

const TITLE = "PointHand Background Test";
const form = [
  "Add-Type -AssemblyName System.Windows.Forms",
  "$f = New-Object Windows.Forms.Form; $f.Text = '" + TITLE + "'; $f.Width = 600; $f.Height = 380; $f.StartPosition = 'Manual'; $f.Left = 40; $f.Top = 40",
  "$t = New-Object Windows.Forms.TextBox; $t.Multiline = $true; $t.AccessibleName = 'notes'; $t.SetBounds(10, 10, 560, 180)",
  "$c = New-Object Windows.Forms.CheckBox; $c.Text = 'agree'; $c.SetBounds(10, 200, 200, 30)",
  "$b = New-Object Windows.Forms.Button; $b.Text = 'count'; $b.SetBounds(10, 240, 100, 30)",
  "$l = New-Object Windows.Forms.Label; $l.Text = '0'; $l.AccessibleName = 'counter'; $l.SetBounds(130, 245, 100, 30)",
  "$b.Add_Click({ $l.Text = [string]([int]$l.Text + 1); $l.AccessibleName = 'counter ' + $l.Text })",
  "$f.Controls.AddRange(@($t, $c, $b, $l)); [Windows.Forms.Application]::Run($f)",
].join("; ");

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

async function call(client, name, args) {
  const result = await client.callTool({ name, arguments: args });
  const part = result.content.find((item) => item.type === "text");
  const body = part ? part.text : "";
  if (result.isError) throw new Error(`${name}: ${body}`);
  return JSON.parse(body);
}

async function find(client, windowId, filter) {
  const found = await call(client, "find_elements", { window_id: windowId, ...filter });
  assert.ok(found.matches.length > 0, `no element for ${JSON.stringify(filter)}`);
  return found.matches[0];
}

const pause = (ms) => new Promise((done) => setTimeout(done, ms));
const target = spawn("powershell.exe", ["-NoProfile", "-STA", "-Command", form], { stdio: "ignore" });
const env = { ...process.env, POINTHAND_ALLOW: "powershell.exe", POINTHAND_OVERLAY: "0" };
delete env.POINTHAND_HOST;
delete env.POINTHAND_BACKGROUND;
const client = new Client({ name: "pointhand-background-e2e", version: "1" });
const report = { checks: {} };
try {
  await client.connect(
    new StdioClientTransport({ command: process.execPath, args: [resolve(root, "client/dist/index.js")], cwd: root, env }),
  );
  report.tools = (await client.listTools()).tools.length;

  let windowId;
  for (let attempt = 0; attempt < 30 && !windowId; attempt++) {
    const rows = await call(client, "list_windows", { query: TITLE });
    windowId = rows[0]?.id;
    if (!windowId) await pause(500);
  }
  assert.ok(windowId, "test window did not appear");
  // Opening the test window may take focus once. From here nothing may move.
  await pause(800);
  // Opening the window may take focus once. Minimize and restore it without
  // activation so it sits behind the user's window, then nothing may raise it.
  // A normal minimize hands activation to the next window, so the target ends
  // up behind the user's work. If the user is busy, fall back to background.
  await call(client, "set_window_state", { window_id: windowId, state: "minimize" }).catch(() =>
    call(client, "set_window_state", { window_id: windowId, state: "minimize", background: true }),
  );
  await call(client, "set_window_state", { window_id: windowId, state: "restore", background: true });
  await pause(500);
  const start = desktopState();
  report.target_behind_at_start = start.foreground !== windowId;

  const notes = await find(client, windowId, { name: "notes", role: "Edit" });
  await call(client, "click", { element_index: notes.element_index, background: true });
  await call(client, "type_text", { window_id: windowId, text: "posted ğüşİ", background: true });
  await pause(300);
  report.checks.posted_typing = (await call(client, "get_element", { element_index: notes.element_index })).value;

  await call(client, "set_value", { element_index: notes.element_index, value: "set by UIA", background: true });
  report.checks.set_value = (await call(client, "get_element", { element_index: notes.element_index })).value;

  const agree = await find(client, windowId, { name: "agree" });
  report.checks.toggle = (
    await call(client, "perform_secondary_action", { element_index: agree.element_index, action: "Toggle", background: true })
  ).state;

  const count = await find(client, windowId, { name: "count", role: "Button" });
  report.checks.button_delivery = (await call(client, "click", { element_index: count.element_index, background: true })).delivery;
  await pause(300);
  report.checks.posted_button_click = (await find(client, windowId, { name: "counter" })).line;

  const state = await call(client, "get_window_state", { window_id: windowId, include_screenshot: true, background: true });
  report.checks.capture = state.screenshot?.capture ?? null;

  const after = desktopState();
  // The user may keep working, so their own clicks can move the pointer or
  // the foreground. What background mode must never do is pull the target forward.
  report.target_stayed_behind = after.foreground !== windowId;
  report.pointer_moved_by_anyone = !(after.x === start.x && after.y === start.y);
  assert.equal(report.checks.posted_typing, "posted ğüşİ");
  assert.equal(report.checks.set_value, "set by UIA");
  assert.equal(report.checks.toggle, "checked");
  assert.match(report.checks.posted_button_click, /counter 1/);
  assert.ok(report.target_stayed_behind, "background mode brought the target window forward");
  report.ok = true;
} catch (error) {
  report.ok = false;
  report.error = error instanceof Error ? error.message : String(error);
  process.exitCode = 1;
} finally {
  await client.close().catch(() => {});
  target.kill();
  console.log(JSON.stringify(report, null, 2));
}
