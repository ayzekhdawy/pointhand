import { test } from "node:test";
import assert from "node:assert/strict";
import { compareVersions, pickLatest, releaseHighlights, updateNotice } from "../dist/update.js";

test("prerelease numbers and final releases order correctly", () => {
  assert.equal(compareVersions("0.2.0-alpha.6", "v0.2.0-alpha.5"), 1);
  assert.equal(compareVersions("0.2.0-alpha.10", "0.2.0-alpha.9"), 1);
  assert.equal(compareVersions("0.2.0", "0.2.0-alpha.9"), 1);
  assert.equal(compareVersions("0.2.0-alpha.5", "0.2.0-beta.1"), -1);
  assert.equal(compareVersions("0.3.0-alpha.1", "0.2.9"), 1);
  assert.equal(compareVersions("v0.2.0-alpha.5", "0.2.0-alpha.5"), 0);
});

test("the newest published release wins and drafts are ignored", () => {
  const releases = [
    { tag_name: "v0.2.0-alpha.5", html_url: "https://x/5", body: "- old" },
    { tag_name: "v0.2.0-alpha.7", html_url: "https://x/7", body: "", draft: true },
    { tag_name: "v0.2.0-alpha.6", html_url: "https://x/6", body: "# Notes\n- **Background** mode\n- `find_elements`\ntext" },
  ];
  const info = pickLatest(releases, "0.2.0-alpha.5", new Date(0));
  assert.equal(info.latest, "0.2.0-alpha.6");
  assert.equal(info.newer, true);
  assert.equal(info.url, "https://x/6");
  assert.deepEqual(info.highlights, ["Background mode", "find_elements"]);
  assert.equal(pickLatest(releases, "0.2.0-alpha.6").newer, false);
  assert.equal(pickLatest([]), null);
});

test("highlights keep only the first bullet lines", () => {
  const body = Array.from({ length: 10 }, (_, i) => `- item ${i}`).join("\n");
  assert.equal(releaseHighlights(body).length, 6);
});

test("the notice names both versions and the release page", () => {
  const text = updateNotice({ current: "0.2.0-alpha.5", latest: "0.2.0-alpha.6", newer: true, url: "https://x/6", highlights: ["A"], checked_at: "" });
  assert.match(text, /0\.2\.0-alpha\.5 -> 0\.2\.0-alpha\.6/);
  assert.match(text, /What is new: A/);
  assert.match(text, /https:\/\/x\/6/);
});
