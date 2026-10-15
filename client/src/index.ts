#!/usr/bin/env node
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";

import { ComputerUseHost } from "./host.js";
import { normalizeParams } from "./compat.js";
import { shapePoints } from "./shapes.js";

const host = new ComputerUseHost(ComputerUseHost.locate());
const server = new McpServer({ name: "pointhand", version: "0.2.0" });

type ToolResult = {
  content: Array<
    | { type: "text"; text: string }
    | { type: "image"; data: string; mimeType: string }
  >;
  isError?: boolean;
};

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function present(value: unknown): ToolResult {
  const record = asRecord(value);
  const shot = asRecord(record?.screenshot);
  const png = typeof shot?.png_base64 === "string" ? shot.png_base64 : null;
  if (record && shot && png) {
    const copy = { ...record, screenshot: { ...shot } };
    delete (copy.screenshot as Record<string, unknown>).png_base64;
    return {
      content: [
        { type: "text", text: JSON.stringify(copy) },
        { type: "image", data: png, mimeType: "image/png" },
      ],
    };
  }
  return { content: [{ type: "text", text: JSON.stringify(value) }] };
}

async function run(method: string, params: unknown, timeoutMs = 20_000): Promise<ToolResult> {
  try {
    return present(await host.call(method, normalizeParams(method, params), timeoutMs));
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

server.registerTool("get_system_info", {
  description: "Physical-pixel virtual desktop bounds and host architecture.",
  inputSchema: {},
}, async () => run("get_system_info", {}));

const windowId = z.number().int().positive().describe("Window id from list_windows.");
const window = z.object({ id: z.number().int().positive(), app: z.string().optional(), title: z.string().optional() }).optional();
const target = { window_id: windowId.optional(), window };
const point = {
  motion: z.enum(["natural", "instant"]).optional().describe("Natural eases pointer acceleration and deceleration; instant moves directly. Drawing paths remain exact."),
  x: z.number().int().optional().describe("X in window pixels when window_id is set, otherwise screen pixels."),
  y: z.number().int().optional().describe("Y in window pixels when window_id is set, otherwise screen pixels."),
  ...target,
  element_index: z.number().int().optional().describe("Index from the latest accessibility tree. Preferred over x,y."),
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
    description: "Visible top-level windows. Each id is the window handle to pass to other tools.",
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
      "Foreground the window, then return its rectangle, an optional accessibility tree, and an optional PNG. " +
      "Coordinates in the image are window pixels divided by screenshot.scale. " +
      "The shot is the visible desktop at the window rectangle; pixels hidden behind other windows are whatever is in front.",
    inputSchema: {
      ...target,
      include_screenshot: z.boolean().optional(),
      include_text: z.boolean().optional(),
      scale: z.number().optional().describe("Longest-edge scale, capped so the image stays under 1568px. Default 0.5."),
    },
  },
  async (args) => run("get_window_state", args, 30_000),
);

server.registerTool(
  "click",
  {
    description: "Click an accessibility element, a point inside a window, or a screen point. button is left, right, or middle.",
    inputSchema: {
      ...point,
      button: z.enum(["left", "right", "middle"]).optional(),
      mouse_button: z.enum(["left", "right", "middle", "l", "r", "m"]).optional(),
      click_count: z.number().int().min(1).max(2).optional(),
      count: z.number().int().optional().describe("2 double-clicks."),
    },
  },
  async (args) => run("click", args),
);

server.registerTool(
  "draw",
  {
    description: "Hold the left button and trace a polyline. points are [x, y] pairs, window pixels when window_id is set. Use this for freehand drawing.",
    inputSchema: {
      ...target,
      points: z.array(z.tuple([z.number().int(), z.number().int()])).min(2),
      step: z.number().int().optional().describe("Maximum pixels between samples. Default 6."),
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
      "Coordinates are window pixels when window_id is set. Select a pen or brush in the app first.",
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
      return await run("draw", { window_id: args.window_id, window: args.window, points, step: args.step }, 60_000);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      return { content: [{ type: "text", text: message }], isError: true };
    }
  },
);

server.registerTool("draw_strokes", {
  description: "Trace up to 2000 independent straight strokes in one target window. Each stroke is [x1,y1,x2,y2] in window pixels. Select the pen and color first. Escape interrupts and releases the button.",
  inputSchema: {
    ...target,
    strokes: z.array(z.tuple([z.number().int(),z.number().int(),z.number().int(),z.number().int()])).min(1).max(2000),
    hold_ms: z.number().int().min(0).max(500).optional().describe("Pause after pressing and before releasing each stroke. Raise it for apps that read the cursor late (Illustrator). Default 0."),
    settle_ms: z.number().int().min(0).max(1000).optional().describe("Pause between strokes. Default 3."),
    step: z.number().int().min(0).max(200).optional().describe("Walk the pointer in steps of this many pixels. 0 (default) sends only the end points; use 6-12 for Illustrator."),
  },
}, async args => run("draw_strokes",args,600_000));

server.registerTool(
  "screenshot",
  {
    description:
      "Screenshot of the whole desktop or a region. region is [left, top, right, bottom] in screen pixels. " +
      "The result states the scale; screen = region origin + image pixel / scale.",
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
    description: "Move the pointer to an accessibility element, a point inside a window, or a screen point without clicking.",
    inputSchema: { ...point },
  },
  async (args) => run("move", args),
);

server.registerTool(
  "drag",
  {
    description: "Drag with the left button. from_x/from_y/to_x/to_y are window pixels when window_id is set.",
    inputSchema: {
      ...target,
      from_x: z.number().int(),
      from_y: z.number().int(),
      to_x: z.number().int(),
      to_y: z.number().int(),
    },
  },
  async (args) => run("drag", args),
);

server.registerTool(
  "scroll",
  {
    description: "Move to a point and scroll. Positive scrollY moves the page down. Positive scrollX moves right.",
    inputSchema: {
      ...point,
      scrollX: z.number().int().optional(),
      scrollY: z.number().int().optional(),
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
    },
  },
  async (args) => run("set_value", args),
);

server.registerTool(
  "perform_secondary_action",
  {
    description: "Invoke, Expand, Collapse, Raise, or Scroll Up/Down/Left/Right on an accessibility element.",
    inputSchema: {
      ...target,
      element_index: z.number().int(),
      action: z.string(),
    },
  },
  async (args) => run("perform_secondary_action", args),
);

server.registerTool(
  "paste",
  {
    description: "Write text to the clipboard and press Ctrl+V. The previous clipboard contents stay replaced.",
    inputSchema: {
      text: z.string(),
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
  "batch",
  {
    description:
      "Run up to 50 actions in order and stop at the first error. " +
      "Actions: click, double_click, right_click, move, drag, draw, scroll, type, key, set_value, secondary, paste, focus_window, wait. " +
      "Input waits until the user has been idle, and Escape stops further actions.",
    inputSchema: {
      actions: z.array(z.record(z.string(), z.unknown())).max(50),
    },
  },
  async (args) => run("batch", args, 60_000),
);

const transport = new StdioServerTransport();
await server.connect(transport);
server.server.onclose = () => { void host.close(); };
process.on("SIGINT", () => { void server.close().finally(() => host.close()); });
process.on("SIGTERM", () => { void server.close().finally(() => host.close()); });
