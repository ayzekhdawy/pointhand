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
  if (method === "batch" && Array.isArray(result.actions)) {
    result.actions = result.actions.map(action => normalizeParams("action", action));
  }
  return result;
}
