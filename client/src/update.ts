import fs from "node:fs";
import os from "node:os";
import path from "node:path";

/** The version this package ships as. Bump it with every release tag. */
export const VERSION = "0.2.0-alpha.6";

const RELEASES_URL = "https://api.github.com/repos/ayzekhdawy/pointhand/releases?per_page=10";
const CACHE_MS = 12 * 60 * 60 * 1000;
const FETCH_TIMEOUT_MS = 5_000;
const MAX_HIGHLIGHTS = 6;

export interface UpdateInfo {
  current: string;
  latest: string;
  newer: boolean;
  url: string;
  highlights: string[];
  checked_at: string;
}

interface Release {
  tag_name?: unknown;
  html_url?: unknown;
  body?: unknown;
  draft?: unknown;
}

/** -1, 0 or 1. Understands 0.2.0, v0.2.0-alpha.5 and 1.0.0-beta.2; a release beats its prereleases. */
export function compareVersions(a: string, b: string): number {
  const parse = (text: string) => {
    const [core, pre = ""] = text.trim().replace(/^v/i, "").split("-", 2);
    return { core: core.split(".").map((part) => Number.parseInt(part, 10) || 0), pre };
  };
  const left = parse(a);
  const right = parse(b);
  for (let i = 0; i < 3; i++) {
    const diff = (left.core[i] ?? 0) - (right.core[i] ?? 0);
    if (diff !== 0) return Math.sign(diff);
  }
  if (left.pre === right.pre) return 0;
  if (!left.pre) return 1;
  if (!right.pre) return -1;
  const leftParts = left.pre.split(".");
  const rightParts = right.pre.split(".");
  for (let i = 0; i < Math.max(leftParts.length, rightParts.length); i++) {
    const l = leftParts[i];
    const r = rightParts[i];
    if (l === undefined) return -1;
    if (r === undefined) return 1;
    const ln = /^\d+$/.test(l) ? Number(l) : NaN;
    const rn = /^\d+$/.test(r) ? Number(r) : NaN;
    if (!Number.isNaN(ln) && !Number.isNaN(rn)) {
      if (ln !== rn) return Math.sign(ln - rn);
    } else if (l !== r) {
      return l < r ? -1 : 1;
    }
  }
  return 0;
}

/** The first bullet lines of release notes, without markdown markers. */
export function releaseHighlights(body: string): string[] {
  return body
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => /^[-*] /.test(line))
    .map((line) => line.slice(2).replace(/\*\*|`/g, "").trim())
    .filter((line) => line.length > 0)
    .slice(0, MAX_HIGHLIGHTS)
    .map((line) => (line.length > 200 ? `${line.slice(0, 197)}...` : line));
}

/** Newest published release, prereleases included while PointHand is in alpha. */
export function pickLatest(releases: Release[], current = VERSION, now = new Date()): UpdateInfo | null {
  const published = releases.filter((item) => item.draft !== true && typeof item.tag_name === "string");
  if (published.length === 0) return null;
  published.sort((a, b) => compareVersions(String(b.tag_name), String(a.tag_name)));
  const latest = published[0];
  const tag = String(latest.tag_name).replace(/^v/i, "");
  return {
    current,
    latest: tag,
    newer: compareVersions(tag, current) > 0,
    url: typeof latest.html_url === "string" ? latest.html_url : "https://github.com/ayzekhdawy/pointhand/releases",
    highlights: typeof latest.body === "string" ? releaseHighlights(latest.body) : [],
    checked_at: now.toISOString(),
  };
}

/** One text line an agent can relay to the user. */
export function updateNotice(info: UpdateInfo): string {
  const lines = [
    `PointHand update available: ${info.current} -> ${info.latest}. Tell the user once; PointHand never updates itself.`,
  ];
  if (info.highlights.length > 0) lines.push(`What is new: ${info.highlights.join(" | ")}`);
  lines.push(`Download and verify the SHA-256 from ${info.url}`);
  return lines.join("\n");
}

function cacheFile(): string {
  const base = process.env.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local");
  return path.join(base, "PointHand", "update-check.json");
}

function readCache(now: number): UpdateInfo | null {
  try {
    const cached = JSON.parse(fs.readFileSync(cacheFile(), "utf8")) as UpdateInfo;
    if (now - Date.parse(cached.checked_at) > CACHE_MS) return null;
    // A newer local install makes an old cached answer wrong.
    return { ...cached, current: VERSION, newer: compareVersions(cached.latest, VERSION) > 0 };
  } catch {
    return null;
  }
}

function writeCache(info: UpdateInfo): void {
  try {
    fs.mkdirSync(path.dirname(cacheFile()), { recursive: true });
    fs.writeFileSync(cacheFile(), JSON.stringify(info));
  } catch {
    // A read-only profile only loses the cache.
  }
}

export function updateCheckEnabled(): boolean {
  return !["0", "false", "off", "no"].includes((process.env.POINTHAND_UPDATE_CHECK ?? "").trim().toLowerCase());
}

/**
 * Ask GitHub for the newest release, at most every 12 hours. Sends no data
 * beyond the request itself. Returns null when disabled, offline or unknown.
 */
export async function checkForUpdate(force = false): Promise<UpdateInfo | null> {
  if (!updateCheckEnabled()) return null;
  const now = Date.now();
  if (!force) {
    const cached = readCache(now);
    if (cached) return cached;
  }
  try {
    const response = await fetch(RELEASES_URL, {
      headers: { "User-Agent": `pointhand/${VERSION}`, Accept: "application/vnd.github+json" },
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
    if (!response.ok) return null;
    const info = pickLatest((await response.json()) as Release[]);
    if (info) writeCache(info);
    return info;
  } catch {
    return null;
  }
}
