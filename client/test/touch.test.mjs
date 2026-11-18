// A control that shows only on hover must also show on each device with a touch
// screen, also a laptop with a touch screen and a mouse (issue #528). Such a
// device matches `(any-pointer: coarse)` but not `(hover: none)`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";

const dir = new URL("../src/", import.meta.url);
const files = readdirSync(dir, { recursive: true }).filter((f) => /\.(svelte|css)$/.test(f));

test("each (hover: none) media query also matches a touch screen with a mouse", () => {
  let found = 0;
  for (const f of files) {
    const text = readFileSync(new URL(f, dir), "utf8");
    for (const [query] of text.matchAll(/@media[^{]*\(hover:\s*none\)[^{]*/g)) {
      found++;
      assert.match(query, /\(any-pointer:\s*coarse\)/, `${f}: ${query.trim()}`);
    }
  }
  assert.ok(found >= 2, "the brief line and the watch list each have one");
});

test("each touch size rule matches any touch screen, also next to a mouse", () => {
  for (const f of files) {
    const text = readFileSync(new URL(f, dir), "utf8");
    assert.doesNotMatch(text, /[^-]pointer:\s*coarse/, `${f}: use (any-pointer: coarse), not (pointer: coarse)`);
  }
});
