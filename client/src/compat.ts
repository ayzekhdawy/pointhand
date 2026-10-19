function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

/** Accept window2-style targets while retaining the original MCP parameters. */
export function normalizeParams(method: string, params: unknown): unknown {
  if (!params || typeof params !== "object" || Array.isArray(params)) return params;
  const result = { ...params } as Record<string, unknown>;
  if (result.window !== undefined && (!result.window || typeof result.window !== "object" || Array.isArray(result.window) || !Number.isSafeInteger((result.window as {id?: unknown}).id))) {
    throw new Error("window must contain a valid integer id");
  }
  if (result.window_id !== undefined && (!Number.isSafeInteger(result.window_id) || Number(result.window_id) <= 0)) throw new Error("window_id must be a positive integer");
  const window = result.window as { id?: unknown } | undefined;
  const id = window?.id ?? (method === "get_window" ? result.id : undefined);
  if (id !== undefined) {
    if (!Number.isSafeInteger(id) || Number(id) <= 0) throw new Error("window.id must be an integer");
    if (result.window_id !== undefined && result.window_id !== id) {
      throw new Error("window and window_id refer to different windows");
    }
    result.window_id = id;
  }
  if (result.mouse_button !== undefined && result.button === undefined) result.button = result.mouse_button;
  if (result.click_count !== undefined && result.count === undefined) result.count = result.click_count;
  if (result.screenshotId === undefined && result.screenshot_id !== undefined) result.screenshotId = result.screenshot_id;
  if (result.screenshotId !== undefined) {
    if (typeof result.screenshotId !== "string" || result.screenshotId.trim() === "") {
      throw new Error("screenshotId must be a non-empty string");
    }
    result.screenshotId = result.screenshotId.trim();
  }
  if (method === "batch" && Array.isArray(result.actions)) {
    result.actions = result.actions.map(action => normalizeParams("action", action));
  }
  return result;
}

function imageBytes(value: unknown): string | undefined {
  if (typeof value !== "string" || value.length === 0) return undefined;
  const marker = "base64,";
  const at = value.indexOf(marker);
  return at >= 0 ? value.slice(at + marker.length) : value;
}

/** A pointer action that refused to click a different window. The image, when present, is the target. */
export function coveredRefusal(value: unknown): boolean {
  const record = asRecord(value);
  return record?.acted === false && record?.reason === "covered";
}

/** Drop raw image and WAV bytes from the JSON text. The caller attaches each part. */
export function presentPayload(value: unknown): { text: string; pngBase64?: string; images: string[]; audioBase64?: string } {
  const record = asRecord(value);
  if (!record) return { text: JSON.stringify(value), images: [] };
  const shot = asRecord(record.screenshot);
  const png = typeof shot?.png_base64 === "string" ? shot.png_base64 : undefined;
  const shots = record.screenshots;
  const copy: Record<string, unknown> = { ...record };
  if (shot && png) {
    const next = { ...shot };
    delete next.png_base64;
    copy.screenshot = next;
  }
  const images: string[] = [];
  if (Array.isArray(shots)) {
    copy.screenshots = shots.map((item) => {
      const row = asRecord(item);
      if (!row) return item;
      const bytes = imageBytes(row.url);
      if (bytes && !images.includes(bytes)) images.push(bytes);
      const next = { ...row };
      delete next.url;
      return next;
    });
  }
  if (images.length === 0 && png) images.push(png);
  let audioBase64: string | undefined;
  if (typeof copy.data_url === "string" && copy.data_url.startsWith("data:audio/")) {
    audioBase64 = imageBytes(copy.data_url);
    delete copy.data_url;
    delete copy.bytes;
  }
  return { text: JSON.stringify(copy), pngBase64: images[0], images, audioBase64 };
}
