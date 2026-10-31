#!/usr/bin/env node
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";

import { ComputerUseHost } from "./host.js";
import { coveredRefusal, normalizeParams, presentPayload } from "./compat.js";
import { shapePoints } from "./shapes.js";

const host = new ComputerUseHost(ComputerUseHost.locate());
const server = new McpServer({ name: "pointhand", version: "0.2.0" });

type ToolResult = {
  content: Array<
    | { type: "text"; text: string }
    | { type: "image"; data: string; mimeType: string }
    | { type: "audio"; data: string; mimeType: string }
  >;
  isError?: boolean;
};

function present(value: unknown): ToolResult {
  const payload = presentPayload(value);
  const content: ToolResult["content"] = [{ type: "text", text: payload.text }];
  for (const data of payload.images) content.push({ type: "image", data, mimeType: "image/png" });
  if (payload.audioBase64) content.push({ type: "audio", data: payload.audioBase64, mimeType: "audio/wav" });
  return { content };
}

async function run(method: string, params: unknown, timeoutMs = 20_000): Promise<ToolResult> {
  try {
    const value = await host.call(method, normalizeParams(method, params), timeoutMs);
    const presented = present(value);
    if (coveredRefusal(value)) return { ...presented, isError: true };
    return presented;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return { content: [{ type: "text", text: message }], isError: true };
  }
}

server.registerTool("overlay_status", {
  description: "PointHand native Rust overlay readiness, frame timing and renderer errors.",
  inputSchema: {},
}, async () => run("overlay_status", {}));

server.registerTool("guard_status", {
  description: "Physical-input monitor readiness and active desktop availability; no keystroke content is recorded.",
  inputSchema: {},
}, async () => run("guard_status", {}));

server.registerTool("emergency_stop", {
  description: "Stop PointHand immediately, including an active drawing or batch, and release injected inputs. Bypasses the input queue. Restart the MCP server to resume; guard_status remains available.",
  inputSchema: {},
}, async () => {
  try {
    await host.requestStop();
    return present({ stop_requested: true, restart_required: true });
  } catch (error) {
    return { content: [{ type: "text" as const, text: error instanceof Error ? error.message : String(error) }], isError: true };
  }
});

server.registerTool("get_system_info", {
  description: "Physical-pixel virtual desktop bounds, monitor/work-area rectangles, per-monitor effective DPI and host architecture.",
  inputSchema: {},
}, async () => run("get_system_info", {}));

const windowId = z.number().int().positive().describe("Window id from list_windows.");
const background = z.boolean().optional().describe(
  "Work without touching the user's mouse, keyboard or focus, so they can keep working in another window. " +
  "Needs a target window. Pointer and key input is posted to that window even when it is covered; it must not be minimized. " +
  "Classic Win32 controls accept it; Chromium, Electron and some modern apps may ignore it, so verify with get_window_state. " +
  "Skips the wait for the user to be idle. The allow-list and physical Escape still apply. Default from POINTHAND_BACKGROUND.",
);
const window = z.object({ id: z.number().int().positive(), app: z.string().optional(), title: z.string().optional() }).optional();
const target = { window_id: windowId.optional(), window };
const point = {
  motion: z.enum(["natural", "instant"]).optional().describe("Natural eases pointer acceleration and deceleration; instant moves directly. Drawing paths remain exact."),
  x: z.number().int().optional().describe("Physical pixels inside the screenshotId region, window pixels when only window_id is set, otherwise screen pixels."),
  y: z.number().int().optional().describe("Physical pixels inside the screenshotId region, window pixels when only window_id is set, otherwise screen pixels."),
  screenshotId: z.string().min(1).optional().describe("Window, monitor slice, owned popup, or monitor screenshot. x and y are physical pixels inside that region."),
  ...target,
  element_index: z.number().int().optional().describe("Index from the latest accessibility tree. Preferred over x,y."),
  background,
};

server.registerTool(
  "list_apps",
  {
    description: "Running apps grouped by executable. Set include_start_menu to also search Start Menu shortcuts.",
    inputSchema: {
      query: z.string().optional(),
      include_start_menu: z.boolean().optional(),
    },
  },
  async (args) => run("list_apps", args),
);

server.registerTool(
  "list_windows",
  {
    description: "Visible top-level windows. Each id is the window handle to pass to other tools. monitors lists each display the window rectangle overlaps.",
    inputSchema: { query: z.string().optional() },
  },
  async (args) => run("list_windows", args),
);

server.registerTool(
  "get_window",
  {
    description: "Title, process path, and screen rectangle of one window.",
    inputSchema: { ...target, id: windowId.optional() },
  },
  async (args) => run("get_window", args),
);

server.registerTool(
  "launch_app",
  {
    description: "Open an executable path, a Start Menu shortcut, or a display name such as Notepad.",
    inputSchema: { app: z.string() },
  },
  async (args) => run("launch_app", args, 25_000),
);

server.registerTool(
  "activate_window",
  {
    description: "Bring a window to the foreground. Refuses while the user is actively typing or moving the mouse.",
    inputSchema: { ...target },
  },
  async (args) => run("activate_window", args),
);

server.registerTool(
  "get_window_state",
  {
    description:
      "Return the window rectangle, the monitors it touches, optional accessibility fields, and optional PNGs. " +
      "A window screenshot uses that window's own pixels when Windows can capture it, including when another window covers it, and does not pull the window forward. " +
      "capture is \"window\" or \"visible\". visible means graphics capture was unavailable, so the image is the desktop and the window may be raised. " +
      "A window that crosses monitors also returns one screenshot per monitor intersection. Owned popups are separate regions. " +
      "Pass a region's screenshotId to click, drag, scroll, or move. x and y are physical pixels inside that region, not image pixels: divide image pixels by scale. " +
      "The id must belong to the latest capture, and the window or popup must still be where it was captured. " +
      "After a recent action the host waits about 1 second, and up to 5 seconds more while a progress indicator is visible or the layout is still changing. Do not insert your own delay. The result includes waited_ms. " +
      "With include_text, a line lists actions=\"Invoke, Expand\" for the secondary actions that element can run. " +
      "Check boxes and toggle buttons show checked, unchecked or mixed. A password field shows password and never its value.",
    inputSchema: {
      ...target,
      include_screenshot: z.boolean().optional(),
      include_text: z.boolean().optional(),
      scale: z.number().optional().describe("Longest-edge scale, capped so the image stays under 1568px. Default 0.5."),
      background: z.boolean().optional().describe("Do not wait for the user to stop typing before a window capture."),
    },
  },
  async (args) => run("get_window_state", args, 30_000),
);

server.registerTool(
  "click",
  {
    description: "Click an accessibility element, a point inside a window, or a screen point. button is left, right, or middle. screenshotId selects a window, monitor slice, owned popup, or monitor screenshot. x and y are physical pixels inside that region. A pointer action brings the target window forward. If a different window still covers the point, no input is sent: the error names that process and includes a fresh screenshot of the target when capture works. Retry the input once with that new screenshotId. Do not click the covering window.",
    inputSchema: {
      ...point,
      button: z.enum(["left", "right", "middle"]).optional(),
      mouse_button: z.enum(["left", "right", "middle", "l", "r", "m"]).optional(),
      click_count: z.number().int().min(1).max(3).optional(),
      count: z.number().int().optional().describe("2 double-clicks, 3 triple-clicks (selects a line or paragraph in most editors)."),
      screenshotId: z.string().min(1).optional(),
    },
  },
  async (args) => run("click", args),
);

server.registerTool(
  "draw",
  {
    description: "Hold the left button and trace a polyline. points are [x, y] pairs, window pixels when window_id is set. Use this for freehand drawing. If another window covers the first point, the stroke is not sent. Retry once with the fresh target screenshot in the error.",
    inputSchema: {
      ...target,
      points: z.array(z.tuple([z.number().int(), z.number().int()])).min(2),
      step: z.number().int().optional().describe("Maximum pixels between samples. Default 6."),
      background,
    },
  },
  async (args) => run("draw", args, 60_000),
);

server.registerTool(
  "draw_shape",
  {
    description:
      "Draw a geometric shape with the left button held: line, ellipse (use equal width and height for a circle), " +
      "rectangle, polygon, star, heart, spiral, or zigzag. cx,cy is the centre; width/height are the bounding size. " +
      "Coordinates are window pixels when window_id is set. Select a pen or brush in the app first. " +
      "If another window covers the start point, the shape is not drawn. Retry once with the fresh target screenshot in the error.",
    inputSchema: {
      shape: z.enum(["line", "ellipse", "rectangle", "polygon", "star", "heart", "spiral", "zigzag"]),
      cx: z.number().int(),
      cy: z.number().int(),
      width: z.number().int().positive(),
      height: z.number().int().positive().optional().describe("Defaults to width."),
      rotation: z.number().optional().describe("Degrees clockwise."),
      sides: z.number().int().optional().describe("Polygon sides or star points (3-24)."),
      ...target,
      step: z.number().int().optional().describe("Maximum pixels between samples. Default 6."),
      background,
    },
  },
  async (args) => {
    try {
      const points = shapePoints({
        shape: args.shape,
        cx: args.cx,
        cy: args.cy,
        width: args.width,
        height: args.height,
        rotation: args.rotation,
        sides: args.sides,
      });
      return await run("draw", { window_id: args.window_id, window: args.window, points, step: args.step, background: args.background }, 60_000);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      return { content: [{ type: "text", text: message }], isError: true };
    }
  },
);

server.registerTool("draw_strokes", {
  description: "Trace up to 2000 independent straight strokes in one target window. Each stroke is [x1,y1,x2,y2] in window pixels. Select the pen and color first. Escape interrupts and releases the button. If another window covers the first point, no stroke is sent. Retry once with the fresh target screenshot in the error.",
  inputSchema: {
    ...target,
    strokes: z.array(z.tuple([z.number().int(),z.number().int(),z.number().int(),z.number().int()])).min(1).max(2000),
    hold_ms: z.number().int().min(0).max(500).optional().describe("Pause after pressing and before releasing each stroke. Raise it for apps that read the cursor late (Illustrator). Default 0."),
    settle_ms: z.number().int().min(0).max(1000).optional().describe("Pause between strokes. Default 3."),
    step: z.number().int().min(0).max(200).optional().describe("Walk the pointer in steps of this many pixels. 0 (default) sends only the end points; use 6-12 for Illustrator."),
    background,
  },
}, async args => run("draw_strokes",args,600_000));

server.registerTool(
  "screenshot",
  {
    description:
      "Screenshot of the whole desktop or a region. screenshots[0] is the combined virtual desktop, or the requested region. " +
      "Each other monitor is a later entry with its own origin. Pass that screenshotId to click, drag, scroll, or move. " +
      "x and y are physical pixels inside that region. screen = originX + x. Divide image pixels by scale first. " +
      "After a recent action the host waits about 1 second, and up to 5 seconds more while a progress indicator is visible or the layout is still changing. Do not insert your own delay. The result includes waited_ms.",
    inputSchema: {
      scale: z.number().optional().describe("Longest-edge scale, capped so the image stays under 1568px. Default 0.5."),
      region: z.tuple([z.number().int(), z.number().int(), z.number().int(), z.number().int()]).optional(),
    },
  },
  async (args) => run("screenshot", args, 30_000),
);

server.registerTool(
  "move",
  {
    description: "Move the pointer without clicking. screenshotId selects a window, monitor slice, owned popup, or monitor screenshot. x and y are physical pixels inside that region. Without it, they are window pixels when window_id is set, otherwise screen pixels. If a different window still covers the point, the pointer does not move there. Retry once with the fresh target screenshotId in the error.",
    inputSchema: { ...point },
  },
  async (args) => run("move", args),
);

server.registerTool(
  "drag",
  {
    description: "Drag with the left button. With screenshotId, from_x and from_y must lie inside that region. to_x and to_y use the same origin and may leave the region, including onto another monitor. Without it, coordinates are window pixels when window_id is set, otherwise screen pixels. The region must be from the latest capture and still in place. If a different window covers the start point, the drag is not sent. Retry once with the fresh target screenshotId in the error.",
    inputSchema: {
      ...target,
      from_x: z.number().int(),
      from_y: z.number().int(),
      to_x: z.number().int(),
      to_y: z.number().int(),
      screenshotId: z.string().min(1).optional(),
      background: background.describe("Post the drag to window_id in window pixels; screenshotId is not used in background mode."),
    },
  },
  async (args) => run("drag", args),
);

server.registerTool(
  "scroll",
  {
    description: "Move to a point and scroll. Positive scrollY moves the page down. Positive scrollX moves right. With screenshotId, x and y are physical pixels inside that window, monitor slice, owned popup, or monitor. The region must be from the latest capture and still in place. If a different window covers the point, the wheel is not sent. Retry once with the fresh target screenshotId in the error.",
    inputSchema: {
      ...point,
      scrollX: z.number().int().optional(),
      scrollY: z.number().int().optional(),
      screenshotId: z.string().min(1).optional(),
    },
  },
  async (args) => run("scroll", args),
);

server.registerTool(
  "type_text",
  {
    description: "Type Unicode text into the focused control. Newlines become Enter. Does not interpret brace codes.",
    inputSchema: {
      text: z.string(),
      ...target,
      background: background.describe("Post characters to the focused control inside window_id. Click the field with background:true first, or use set_value."),
    },
  },
  async (args) => run("type_text", args),
);

server.registerTool(
  "press_key",
  {
    description: "Press a key or chord. Examples: Return, Control_L+a, alt+F4, KP_0, f5.",
    inputSchema: {
      key: z.string(),
      ...target,
      background: background.describe("Post one key without modifiers to the focused control inside window_id. Chords such as ctrl+s need background:false."),
    },
  },
  async (args) => run("press_key", args),
);

server.registerTool(
  "set_value",
  {
    description: "Set the Value pattern of an element from the latest accessibility snapshot.",
    inputSchema: {
      ...target,
      element_index: z.number().int(),
      value: z.string(),
      background: z.boolean().optional().describe("Set the value without moving focus or waiting for the user to be idle."),
    },
  },
  async (args) => run("set_value", args),
);

server.registerTool(
  "perform_secondary_action",
  {
    description: "Run one actions= label from the latest accessibility tree or find_elements result: Invoke, Toggle, Select, Expand, Collapse, Raise, or Scroll Up, Scroll Down, Scroll Left, Scroll Right. Focus moves keyboard focus to the element. Matching ignores case. Do not guess a label the tree did not list. Toggle returns the new state. A disabled element is refused without sending anything.",
    inputSchema: {
      ...target,
      element_index: z.number().int(),
      action: z.string(),
      background: z.boolean().optional().describe("Run the action without moving focus or waiting for the user to be idle."),
    },
  },
  async (args) => run("perform_secondary_action", args),
);

const elementFilter = {
  name: z.string().optional().describe("Case-insensitive part of the element's accessible name."),
  role: z.string().optional().describe("Control type such as Button, Edit, CheckBox, ListItem, MenuItem. Case and spaces are ignored."),
  automation_id: z.string().optional().describe("Exact UI Automation id."),
};

server.registerTool(
  "find_elements",
  {
    description:
      "Search one window's accessibility tree by name, role or automation_id instead of reading the whole tree. " +
      "Walks up to 3000 elements (get_window_state stops at 400) and returns only the matches with their element_index. " +
      "This replaces the latest accessibility snapshot, so the indices work with click, set_value and perform_secondary_action. " +
      "complete=false means the window had more elements than were walked.",
    inputSchema: {
      window_id: windowId,
      ...elementFilter,
      limit: z.number().int().min(1).max(200).optional().describe("Most matches to return. Default 50."),
    },
  },
  async (args) => run("find_elements", args, 30_000),
);

server.registerTool(
  "get_element",
  {
    description:
      "Read one element from the latest accessibility snapshot live: role, name, automation_id, class, rectangle, enabled, offscreen, focused, value, toggle state and runnable actions. " +
      "A password field reports password=true and never its value. Reads only.",
    inputSchema: {
      ...target,
      element_index: z.number().int().min(0),
    },
  },
  async (args) => run("get_element", args),
);

server.registerTool(
  "wait_for",
  {
    description:
      "Wait until a window whose title contains window_title appears, or an element matching name/role/automation_id appears in window_id. " +
      "With gone=true, wait until it disappears instead. Reads only and sends no input. Polls every 250 ms; timeout_ms defaults to 5000 and stops at 30000. " +
      "Use this after a click that opens a dialog or starts a load instead of a fixed wait. Element matches replace the latest accessibility snapshot. Escape stops the wait.",
    inputSchema: {
      window_title: z.string().optional(),
      window_id: windowId.optional(),
      ...elementFilter,
      gone: z.boolean().optional(),
      timeout_ms: z.number().int().min(0).max(30000).optional(),
    },
  },
  async (args) => run("wait_for", args, 40_000),
);

server.registerTool(
  "set_window_state",
  {
    description: "Minimize, maximize or restore a window. Follows the same allow-list, idle and Escape rules as other input.",
    inputSchema: {
      window_id: windowId,
      state: z.enum(["minimize", "maximize", "restore"]),
    },
  },
  async (args) => run("set_window_state", args),
);

server.registerTool(
  "move_window",
  {
    description:
      "Move or resize a window in physical screen pixels. Omitted values keep the current ones. " +
      "A maximized or minimized window is restored first. The window must stay at least 100 px on each side and overlap the desktop. Does not bring the window forward.",
    inputSchema: {
      window_id: windowId,
      x: z.number().int().optional(),
      y: z.number().int().optional(),
      width: z.number().int().positive().optional(),
      height: z.number().int().positive().optional(),
    },
  },
  async (args) => run("move_window", args),
);

server.registerTool(
  "close_window",
  {
    description:
      "Ask a window to close, like its title-bar X. The app may show a save prompt instead; closed=false then means the window is still open. " +
      "Windows session processes such as explorer.exe are refused. Not available inside batch.",
    inputSchema: { window_id: windowId },
  },
  async (args) => run("close_window", args, 20_000),
);

server.registerTool(
  "paste",
  {
    description: "Write the clipboard and press Ctrl+V. format is text (default), md, or html. md and html also place Windows HTML Format so an editor can paste headings, lists, and links. text and md keep the source as plain text. html's plain fallback has the tags removed. The previous clipboard stays replaced.",
    inputSchema: {
      text: z.string(),
      format: z.enum(["text", "md", "html"]).optional(),
      ...target,
    },
  },
  async (args) => run("paste", args),
);

server.registerTool(
  "clipboard_read",
  {
    description: "Read Unicode text from the clipboard.",
    inputSchema: {},
  },
  async () => run("clipboard_read", {}),
);

server.registerTool(
  "clipboard_write",
  {
    description: "Replace the clipboard with Unicode text.",
    inputSchema: { text: z.string() },
  },
  async (args) => run("clipboard_write", args),
);

server.registerTool(
  "start_audio_recording",
  {
    description: "Start recording what Windows is playing (speaker loopback, not the microphone) while other actions continue. max_duration_ms defaults to 60000 and cannot exceed 300000. One recording at a time. This does not wait for the pointer to be idle.",
    inputSchema: {
      max_duration_ms: z.number().int().positive().max(300000).optional(),
    },
  },
  async (args) => run("start_audio_recording", args),
);

server.registerTool(
  "stop_audio_recording",
  {
    description: "Stop the loopback recording and return a 24 kHz stereo WAV. The filepath is on this computer. Physical Escape ends capture early; call this to collect the partial recording.",
    inputSchema: {},
  },
  async () => run("stop_audio_recording", {}, 60_000),
);

server.registerTool(
  "batch",
  {
    description:
      "Run up to 50 actions in order and stop at the first error. A covered point stops the batch without sending that input; call get_window_state and retry once. " +
      "Actions: click, double_click, triple_click, right_click, move, drag, draw, scroll, type, key, set_value, secondary, toggle, select, paste, focus_window, window_state, move_window, wait, wait_for. " +
      "Input waits until the user has been idle, and Escape stops further actions.",
    inputSchema: {
      actions: z.array(z.record(z.string(), z.unknown())).max(50),
      background: z.boolean().optional().describe("Default background flag for every action that does not set its own."),
    },
  },
  async (args) => run("batch", args, 60_000),
);

const transport = new StdioServerTransport();
await server.connect(transport);
server.server.onclose = () => { void host.close(); };
process.on("SIGINT", () => { void server.close().finally(() => host.close()); });
process.on("SIGTERM", () => { void server.close().finally(() => host.close()); });
