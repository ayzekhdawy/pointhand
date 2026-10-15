import { test } from "node:test";
import assert from "node:assert/strict";
import { normalizeParams } from "../dist/compat.js";
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

test("malformed targets cannot silently become global screen input", () => {
 for(const window of [null, {}, [], "paint", {id:0}, {id:-1}, {id:1.5}]) {
  assert.throws(() => normalizeParams("click", {window,x:10,y:10}), /integer|valid/);
 }
 assert.throws(() => normalizeParams("click", {window_id:0,x:10,y:10}), /positive/);
});
