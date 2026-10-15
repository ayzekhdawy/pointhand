// Paint a stroke plan into Adobe Illustrator through the PointHand MCP.
//
// The artboard is never resized or reconfigured: the plan is scaled and centred
// inside whatever artboard is open. Only the stroke colour, the stroke weight
// and the Pencil tool are set, and only through the Illustrator UI.
//
// Usage:
//   node scripts/illustrator-image.mjs --plan .scratch/mona-lisa-strokes.json [options]
// Options:
//   --window <id>        Illustrator window id (default: first window titled Illustrator)
//   --doc-width <n>      artboard width in document units (default 212.5984)
//   --artboard x,y,w,h   artboard in window pixels (default: detected from a screenshot)
//   --margin <px>        free space kept inside the artboard (default 0)
//   --step <px>          pointer step while stroking (default 24)
//   --settle <ms>        pause between strokes (default 3)
//   --colors a-b         inclusive 1-based colour range (default all)
//   --clear              delete everything on the artboard first (Select > All, Edit > Clear)
//   --dry-run            print the mapping and stop; touches nothing
import { Client } from "../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js";
import { StdioClientTransport } from "../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { aspectMismatch, fitPlan, mapStroke, strokeWeight, zoomFor } from "./lib/fit.mjs";

const here = (p) => fileURLToPath(new URL(p, import.meta.url));
const scratch = here("../.scratch/");
mkdirSync(scratch, { recursive: true });

// UI positions in window pixels, measured on a 1920x1080 maximised Illustrator
// with the Color panel floating at the bottom right. Override with --cal file.json.
const CAL = {
  dockColor: [1500, 601],   // "Renk" icon in the docked panel list: opens the Color panel
  panelProbe: [1250, 780],  // dark when the floating Color panel is open
  strokeSwatch: [1171, 722],
  hexField: [1375, 814],
  weightField: [365, 70],
  focusSpot: [1100, 500],
  menuSelect: [378, 26],
  itemAll: [429, 55],
  menuEdit: [206, 26],
  itemClear: [222, 325],
};

function parseArgs(argv) {
  const out = { flags: new Set(), values: {} };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith("--")) continue;
    const name = arg.slice(2);
    if (["dry-run", "clear"].includes(name)) out.flags.add(name);
    else out.values[name] = argv[++i];
  }
  return out;
}

const args = parseArgs(process.argv.slice(2));
const planPath = args.values.plan;
if (!planPath) throw new Error("--plan is required");
const plan = JSON.parse(readFileSync(planPath, "utf8"));
const docWidth = Number(args.values["doc-width"] ?? 212.5984);
const margin = Number(args.values.margin ?? 0);
const step = Number(args.values.step ?? 24);
const settle = Number(args.values.settle ?? 3);
const [fromColor, toColor] = (args.values.colors ?? `1-${plan.colors.length}`).split("-").map(Number);
if (args.values.cal) Object.assign(CAL, JSON.parse(readFileSync(args.values.cal, "utf8")));

const client = new Client({ name: "pointhand-illustrator-painter", version: "1" });
await client.connect(
  new StdioClientTransport({
    command: process.execPath,
    args: [here("../client/dist/index.js")],
    env: { ...process.env, POINTHAND_ALLOW: "" },
  }),
);

const text = (result) => result.content.find((part) => part.type === "text")?.text ?? "";
async function call(name, params = {}) {
  const result = await client.callTool({ name, arguments: params });
  if (result.isError) throw new Error(`${name}: ${text(result)}`);
  return result;
}

let windowId = args.values.window ? Number(args.values.window) : null;
if (windowId === null) {
  // The title becomes the document name once a file is open, so match the executable.
  const all = JSON.parse(text(await call("list_windows", {})));
  const windows = all.filter((w) => /illustrator\.exe$/i.test(w.app ?? ""));
  if (windows.length === 0) throw new Error("Illustrator is not running or has no visible window");
  windowId = windows[0].id;
}

async function act(actions) {
  const result = await call("batch", { actions });
  const rows = JSON.parse(text(result));
  const failed = rows.find((row) => !row.ok);
  if (failed) throw new Error(`batch action ${failed.index} failed: ${JSON.stringify(failed)}`);
}
const click = ([x, y]) => ({ action: "click", window_id: windowId, x, y });
const key = (name) => ({ action: "key", window_id: windowId, key: name });
const wait = (seconds) => ({ action: "wait", seconds });

function probeRed(file, [x, y], windowSize) {
  const code =
    "import sys;from PIL import Image;im=Image.open(sys.argv[1]).convert('RGB');" +
    "ww,wh=int(sys.argv[4]),int(sys.argv[5]);" +
    "print(im.getpixel((int(int(sys.argv[2])*im.width/ww),int(int(sys.argv[3])*im.height/wh)))[0])";
  const out = execFileSync("python", ["-c", code, file, String(x), String(y), String(windowSize[0]), String(windowSize[1])], {
    encoding: "utf8",
  });
  return Number(out.trim());
}

async function windowShot(file) {
  const result = await call("get_window_state", {
    window_id: windowId,
    include_screenshot: true,
    include_text: true,
    scale: 1,
  });
  const info = JSON.parse(text(result));
  const image = result.content.find((part) => part.type === "image");
  if (image) writeFileSync(file, Buffer.from(image.data, "base64"));
  return info;
}

try {
  // 1. Preflight: a document must be open (the New Document dialog must be gone).
  const shotFile = `${scratch}illustrator-window.png`;
  const state = await windowShot(shotFile);
  if (state.accessibility?.tree?.includes('"Yeni Belge"')) {
    throw new Error("The New Document dialog is still open. Create the document first.");
  }

  // 2. Artboard rectangle in window pixels.
  let artboard;
  if (args.values.artboard) {
    const [x, y, w, h] = args.values.artboard.split(",").map(Number);
    artboard = { x, y, w, h };
  } else {
    const rect = state.window.rect;
    const size = [rect[2] - rect[0], rect[3] - rect[1]];
    artboard = JSON.parse(
      execFileSync("python", [here("find-artboard.py"), shotFile, String(size[0]), String(size[1])], {
        encoding: "utf8",
      }),
    );
  }

  const mapping = fitPlan(plan, artboard, margin);
  const zoom = zoomFor(artboard.w, docWidth);
  const weight = strokeWeight(plan.pen_size, mapping.scale, zoom);
  const mismatch = aspectMismatch(plan, artboard);
  const summary = {
    window: windowId,
    artboard,
    plan_canvas: plan.canvas,
    scale: Number(mapping.scale.toFixed(4)),
    drawn: [Math.round(mapping.drawnW), Math.round(mapping.drawnH)],
    stroke_weight_doc_units: Number(weight.toFixed(3)),
    aspect_mismatch: Number(mismatch.toFixed(2)),
    colors: [fromColor, toColor],
    step,
  };
  console.log(JSON.stringify(summary));
  if (mismatch > 1.25) {
    console.log(`note: plan and artboard aspect ratios differ (${mismatch.toFixed(2)}x); the drawing is letterboxed.`);
  }
  if (args.flags.has("dry-run")) {
    console.log("dry run: nothing was changed");
  } else {
    // 3. Optional clean artboard through the menus (keyboard shortcuts get stuck in panels).
    if (args.flags.has("clear")) {
      await act([
        click(CAL.menuSelect), wait(0.35), click(CAL.itemAll), wait(0.35),
        click(CAL.menuEdit), wait(0.35), click(CAL.itemClear), wait(0.6),
      ]);
    }

    // 4. Colour panel, Pencil tool, stroke colour target, stroke weight.
    const rect = state.window.rect;
    const winSize = [rect[2] - rect[0], rect[3] - rect[1]];
    if (probeRed(shotFile, CAL.panelProbe, winSize) > 70) {
      await act([click(CAL.dockColor), wait(0.8)]); // panel was closed: open it
    }
    const weightText = weight.toFixed(2).replace(".", ","); // Turkish decimal comma
    await act([
      click(CAL.focusSpot), wait(0.3), key("n"), wait(0.4), // keyboard focus must be on the canvas
      click(CAL.strokeSwatch), wait(0.2),
      click(CAL.weightField), key("ctrl+a"),
      { action: "type", text: weightText }, key("Return"), wait(0.2),
    ]);

    // 5. Paint colour by colour.
    const started = performance.now();
    let painted = 0;
    for (let index = fromColor - 1; index < toColor; index += 1) {
      const group = plan.colors[index];
      await act([
        click(CAL.hexField), key("ctrl+a"),
        { action: "type", text: group.hex.replace("#", "") }, key("Return"), wait(0.15),
      ]);
      for (let offset = 0; offset < group.strokes.length; offset += 1500) {
        const strokes = group.strokes
          .slice(offset, offset + 1500)
          .map((stroke) => mapStroke(stroke, mapping))
          .filter(([x1, y1, x2, y2]) => x1 !== x2 || y1 !== y2);
        if (strokes.length === 0) continue;
        const result = await call("draw_strokes", { window_id: windowId, strokes, step, settle_ms: settle });
        painted += JSON.parse(text(result)).strokes;
      }
      console.log(JSON.stringify({
        color: index + 1, of: plan.colors.length, hex: group.hex, strokes: painted,
        seconds: Math.round((performance.now() - started) / 1000),
      }));
    }
    await windowShot(`${scratch}illustrator-result.png`);
    console.log("PointHand Illustrator pass complete");
  }
} finally {
  await client.close();
}
