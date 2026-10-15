/** Pure shape generators. Each returns integer [x, y] pairs for the `draw` tool. */

export type Point = [number, number];

export type ShapeName =
  | "line"
  | "ellipse"
  | "rectangle"
  | "polygon"
  | "star"
  | "heart"
  | "spiral"
  | "zigzag";

export interface ShapeSpec {
  shape: ShapeName;
  cx: number;
  cy: number;
  width: number;
  height?: number;
  /** Degrees, clockwise on screen. */
  rotation?: number;
  /** Polygon sides or star points. */
  sides?: number;
}

const ELLIPSE_SEGMENTS = 64;
const MIN_SIDES = 3;
const MAX_SIDES = 24;
const STAR_INNER_RATIO = 0.45;

function rotate(points: Point[], cx: number, cy: number, degrees: number): Point[] {
  if (degrees % 360 === 0) {
    return points;
  }
  const radians = (degrees * Math.PI) / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  return points.map(([x, y]) => {
    const dx = x - cx;
    const dy = y - cy;
    return [cx + dx * cos - dy * sin, cy + dx * sin + dy * cos] as Point;
  });
}

function toInts(points: Point[]): Point[] {
  return points.map(([x, y]) => [Math.round(x), Math.round(y)] as Point);
}

function clampSides(sides: number | undefined, fallback: number): number {
  const value = Math.round(sides ?? fallback);
  return Math.min(MAX_SIDES, Math.max(MIN_SIDES, value));
}

function regular(cx: number, cy: number, rx: number, ry: number, count: number, offset: number): Point[] {
  const points: Point[] = [];
  for (let i = 0; i < count; i += 1) {
    const angle = offset + (2 * Math.PI * i) / count;
    points.push([cx + rx * Math.cos(angle), cy + ry * Math.sin(angle)]);
  }
  return points;
}

function closed(points: Point[]): Point[] {
  return [...points, points[0]];
}

export function shapePoints(spec: ShapeSpec): Point[] {
  const { cx, cy, width } = spec;
  const height = spec.height ?? width;
  if (!(width > 0) || !(height > 0)) {
    throw new Error("width and height must be positive");
  }
  const rx = width / 2;
  const ry = height / 2;
  let raw: Point[];
  switch (spec.shape) {
    case "line":
      raw = [
        [cx - rx, cy - ry],
        [cx + rx, cy + ry],
      ];
      break;
    case "ellipse":
      raw = closed(regular(cx, cy, rx, ry, ELLIPSE_SEGMENTS, 0));
      break;
    case "rectangle":
      raw = closed([
        [cx - rx, cy - ry],
        [cx + rx, cy - ry],
        [cx + rx, cy + ry],
        [cx - rx, cy + ry],
      ]);
      break;
    case "polygon":
      raw = closed(regular(cx, cy, rx, ry, clampSides(spec.sides, 5), -Math.PI / 2));
      break;
    case "star": {
      const count = clampSides(spec.sides, 5);
      const outer = regular(cx, cy, rx, ry, count, -Math.PI / 2);
      const inner = regular(cx, cy, rx * STAR_INNER_RATIO, ry * STAR_INNER_RATIO, count, -Math.PI / 2 + Math.PI / count);
      const merged: Point[] = [];
      for (let i = 0; i < count; i += 1) {
        merged.push(outer[i], inner[i]);
      }
      raw = closed(merged);
      break;
    }
    case "heart": {
      const steps = 72;
      const points: Point[] = [];
      for (let i = 0; i <= steps; i += 1) {
        const t = (2 * Math.PI * i) / steps;
        const hx = 16 * Math.sin(t) ** 3;
        const hy = -(13 * Math.cos(t) - 5 * Math.cos(2 * t) - 2 * Math.cos(3 * t) - Math.cos(4 * t));
        points.push([cx + (hx / 16) * rx, cy + (hy / 17) * ry]);
      }
      raw = points;
      break;
    }
    case "spiral": {
      const steps = 120;
      const turns = 3;
      const points: Point[] = [];
      for (let i = 0; i <= steps; i += 1) {
        const f = i / steps;
        const angle = 2 * Math.PI * turns * f;
        points.push([cx + rx * f * Math.cos(angle), cy + ry * f * Math.sin(angle)]);
      }
      raw = points;
      break;
    }
    case "zigzag": {
      const teeth = clampSides(spec.sides, 6);
      const points: Point[] = [];
      for (let i = 0; i <= teeth * 2; i += 1) {
        points.push([cx - rx + (width * i) / (teeth * 2), i % 2 === 0 ? cy + ry : cy - ry]);
      }
      raw = points;
      break;
    }
    default:
      throw new Error(`unknown shape: ${String(spec.shape)}`);
  }
  return toInts(rotate(raw, cx, cy, spec.rotation ?? 0));
}
