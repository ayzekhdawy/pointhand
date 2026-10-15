import assert from "node:assert/strict";
import { test } from "node:test";

import { aspectMismatch, fitPlan, mapStroke, strokeWeight, zoomFor } from "../../scripts/lib/fit.mjs";

const plan = { canvas: [430, 640] };

test("plan is scaled to the limiting side and centred", () => {
  // Artboard is 1:2 (portrait). The 2:3 plan is limited by width.
  const board = { x: 100, y: 50, w: 212, h: 424 };
  const fit = fitPlan(plan, board);
  assert.ok(Math.abs(fit.scale - 212 / 430) < 1e-9);
  assert.ok(Math.abs(fit.drawnW - 212) < 1e-6);
  assert.ok(fit.drawnH < board.h);
  assert.ok(Math.abs(fit.offsetX - 100) < 1e-6);
  assert.ok(fit.offsetY > 50, "letterboxed vertically");
});

test("exact-size artboard maps 1:1", () => {
  const fit = fitPlan(plan, { x: 754, y: 284, w: 430, h: 640 });
  assert.equal(fit.scale, 1);
  assert.deepEqual(mapStroke([0, 0, 10, 4], fit), [754, 284, 764, 288]);
});

test("margin keeps the drawing inside the artboard", () => {
  const board = { x: 0, y: 0, w: 430, h: 640 };
  const fit = fitPlan(plan, board, 20);
  assert.ok(fit.offsetX >= 20 && fit.offsetY >= 20);
  assert.ok(fit.offsetX + fit.drawnW <= board.w - 20 + 1e-6);
});

test("stroke weight compensates for scale and zoom", () => {
  const zoom = zoomFor(424, 212.5984); // ~1.994 window px per document px
  const weight = strokeWeight(2, 0.5, zoom);
  assert.ok(Math.abs(weight - (2 * 0.5) / zoom) < 1e-9);
  assert.ok(weight > 0.4 && weight < 0.6);
});

test("aspect mismatch is 1 for equal ratios and grows otherwise", () => {
  assert.ok(Math.abs(aspectMismatch(plan, { w: 430, h: 640 }) - 1) < 1e-9);
  assert.ok(aspectMismatch(plan, { w: 212, h: 424 }) > 1.3);
});

test("bad sizes are rejected", () => {
  assert.throws(() => fitPlan(plan, { x: 0, y: 0, w: 0, h: 10 }));
  assert.throws(() => zoomFor(100, 0));
  assert.throws(() => strokeWeight(2, 1, 0));
});
