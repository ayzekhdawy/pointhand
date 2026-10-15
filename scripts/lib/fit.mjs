// Pure geometry for painting a stroke plan onto an artboard of ANY size.
// The artboard is never resized: the plan is scaled and centred inside it.

/**
 * @param {{canvas:[number,number]}} plan   plan canvas in plan pixels (e.g. 430x640)
 * @param {{x:number,y:number,w:number,h:number}} artboard   artboard rectangle in window pixels
 * @param {number} [margin]   padding in window pixels kept free on every side
 */
export function fitPlan(plan, artboard, margin = 0) {
  const [planW, planH] = plan.canvas;
  const innerW = artboard.w - margin * 2;
  const innerH = artboard.h - margin * 2;
  if (!(planW > 0 && planH > 0 && innerW > 0 && innerH > 0)) {
    throw new Error("plan and artboard must have positive size");
  }
  const scale = Math.min(innerW / planW, innerH / planH);
  const offsetX = artboard.x + margin + (innerW - planW * scale) / 2;
  const offsetY = artboard.y + margin + (innerH - planH * scale) / 2;
  return { scale, offsetX, offsetY, drawnW: planW * scale, drawnH: planH * scale };
}

/** Map one flat [x1,y1,x2,y2,...] stroke from plan pixels to window pixels. */
export function mapStroke(stroke, mapping) {
  return stroke.map((value, index) =>
    Math.round((index % 2 === 0 ? mapping.offsetX : mapping.offsetY) + value * mapping.scale),
  );
}

/**
 * Stroke weight to set in the app, in DOCUMENT units, so one plan pixel
 * (pen_size) covers `scale` window pixels at the current zoom.
 * zoom = window pixels per document unit (artboard window width / document width).
 */
export function strokeWeight(penSize, scale, zoom) {
  if (!(zoom > 0)) throw new Error("zoom must be positive");
  return (penSize * scale) / zoom;
}

/** Window pixels per document unit, from the artboard's measured and nominal width. */
export function zoomFor(artboardWindowWidth, artboardDocumentWidth) {
  if (!(artboardDocumentWidth > 0)) throw new Error("document width must be positive");
  return artboardWindowWidth / artboardDocumentWidth;
}

/** Aspect-ratio mismatch worth warning about (1 = identical). */
export function aspectMismatch(plan, artboard) {
  const planRatio = plan.canvas[0] / plan.canvas[1];
  const boardRatio = artboard.w / artboard.h;
  return Math.max(planRatio / boardRatio, boardRatio / planRatio);
}
