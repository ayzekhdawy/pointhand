import assert from "node:assert/strict";
import { test } from "node:test";

import { shapePoints } from "../dist/shapes.js";

const bounds = (points) => ({
  minX: Math.min(...points.map((p) => p[0])),
  maxX: Math.max(...points.map((p) => p[0])),
  minY: Math.min(...points.map((p) => p[1])),
  maxY: Math.max(...points.map((p) => p[1])),
});

test("rectangle is closed and has the requested bounds", () => {
  const points = shapePoints({ shape: "rectangle", cx: 100, cy: 100, width: 80, height: 40 });
  assert.deepEqual(points[0], points[points.length - 1]);
  assert.deepEqual(bounds(points), { minX: 60, maxX: 140, minY: 80, maxY: 120 });
});

test("circle stays inside its bounding box and closes", () => {
  const points = shapePoints({ shape: "ellipse", cx: 0, cy: 0, width: 100 });
  const b = bounds(points);
  assert.ok(b.minX >= -50 && b.maxX <= 50 && b.minY >= -50 && b.maxY <= 50);
  assert.deepEqual(points[0], points[points.length - 1]);
});

test("star alternates outer and inner points", () => {
  const points = shapePoints({ shape: "star", cx: 0, cy: 0, width: 200, sides: 5 });
  assert.equal(points.length, 11);
  const radius = ([x, y]) => Math.hypot(x, y);
  assert.ok(radius(points[0]) > radius(points[1]));
});

test("rotation by 90 degrees swaps the bounds of a wide rectangle", () => {
  const points = shapePoints({ shape: "rectangle", cx: 0, cy: 0, width: 100, height: 20, rotation: 90 });
  const b = bounds(points);
  assert.equal(b.maxX - b.minX, 20);
  assert.equal(b.maxY - b.minY, 100);
});

test("sides are clamped and bad sizes are rejected", () => {
  assert.equal(shapePoints({ shape: "polygon", cx: 0, cy: 0, width: 50, sides: 1 }).length, 4);
  assert.throws(() => shapePoints({ shape: "ellipse", cx: 0, cy: 0, width: 0 }));
});

test("every point is an integer pair", () => {
  for (const shape of ["line", "ellipse", "rectangle", "polygon", "star", "heart", "spiral", "zigzag"]) {
    for (const [x, y] of shapePoints({ shape, cx: 10, cy: 10, width: 57, height: 33, rotation: 17 })) {
      assert.ok(Number.isInteger(x) && Number.isInteger(y), `${shape} produced a non-integer`);
    }
  }
});
