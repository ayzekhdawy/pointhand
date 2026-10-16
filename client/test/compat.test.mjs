import { test } from "node:test";
import assert from "node:assert/strict";
import { normalizeParams, presentPayload } from "../dist/compat.js";
test("window2 target and click aliases reach the host without mutating arguments", () => {
 const input = { window: { id: 42, app: "paint" }, mouse_button: "r", click_count: 2 };
 const result = normalizeParams("click", input);
 assert.equal(result.window_id, 42); assert.equal(result.button, "r"); assert.equal(result.count, 2);
 assert.equal(input.window_id, undefined);
 assert.equal(normalizeParams("get_window", { id: 42 }).window_id, 42);
});
test("conflicting targets fail before sending input", () => {
 assert.throws(() => normalizeParams("click", { window: {id: 42}, window_id: 43 }), /different windows/);
 assert.equal(normalizeParams("batch", {actions: [{window: {id: 42}}]}).actions[0].window_id, 42);
});

test("screenshotId is trimmed and image bytes stay out of the text payload", () => {
 const result = normalizeParams("click", { screenshot_id: " ph-2 ", window: { id: 7 }, x: 1, y: 2 });
 assert.equal(result.screenshotId, "ph-2");
 assert.equal(result.window_id, 7);
 assert.throws(() => normalizeParams("scroll", { screenshotId: "  " }), /screenshotId/);
 const payload = presentPayload({
  screenshot: { id: "ph-2", png_base64: "QUJD", scale: 0.5, width: 10, height: 8 },
  screenshots: [{ id: "ph-2", url: "data:image/png;base64,QUJD", width: 10, height: 8, originX: 1, originY: 2, zIndex: 0 }],
 });
 assert.equal(payload.pngBase64, "QUJD");
 const text = JSON.parse(payload.text);
 assert.equal(text.screenshot.png_base64, undefined);
 assert.equal(text.screenshot.id, "ph-2");
 assert.equal(text.screenshots[0].url, undefined);
 assert.equal(text.screenshots[0].originX, 1);
 const many = presentPayload({
  screenshot: { id: "ph-1", png_base64: "QUJD" },
  screenshots: [
   { id: "ph-1", url: "data:image/png;base64,QUJD", originX: 0, originY: 0 },
   { id: "ph-2", url: "data:image/png;base64,REVG", originX: -1920, originY: 0, role: "monitor" },
  ],
 });
 assert.deepEqual(many.images, ["QUJD", "REVG"]);
 assert.equal(JSON.parse(many.text).screenshots[1].originX, -1920);
 assert.equal(JSON.parse(many.text).screenshots[1].url, undefined);
});

test("malformed targets cannot silently become global screen input", () => {
 for(const window of [null, {}, [], "paint", {id:0}, {id:-1}, {id:1.5}]) {
  assert.throws(() => normalizeParams("click", {window,x:10,y:10}), /integer|valid/);
 }
 assert.throws(() => normalizeParams("click", {window_id:0,x:10,y:10}), /positive/);
});
