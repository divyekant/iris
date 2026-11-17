// Property tests of the address chips of the write view (client/src/chips.ts):
// the text that the person types or pastes is input that Iris does not control.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load } from "./mount.mjs";

const { chips, unique } = await load("/src/chips.ts");

/** A small xorshift generator: the same seed gives the same cases. */
function rng(seed) {
  let x = seed;
  const next = () => {
    x ^= x << 13;
    x ^= x >>> 17;
    x ^= x << 5;
    return x >>> 0;
  };
  return { below: (n) => next() % n, pick: (xs) => xs[next() % xs.length] };
}

const POOL = ["a@example.com", "A@Example.com", "sam@okafor.example", "not-an-address", "x@y.z", "é@example.com"];
const SEPS = [",", ", ", " ", ";", "\n", ",,", " ; "];

test("each address comes once, in the order of its first entry, and the open end stays text", () => {
  const r = rng(0x9e3779b9);
  for (let n = 0; n < 3000; n++) {
    const list = Array.from({ length: r.below(4) }, () => r.pick(POOL));
    const words = Array.from({ length: r.below(8) }, () => r.pick(POOL));
    let text = words.map((w, i) => (i ? r.pick(SEPS) : "") + w).join("");
    const closed = r.below(2) === 0;
    if (closed && text) text += r.pick(SEPS);
    const end = r.below(3) === 0;
    const [out, rest] = chips(list, text, end);
    const lower = out.map((a) => a.toLowerCase());
    assert.equal(new Set(lower).size, out.length, `duplicate in ${JSON.stringify(out)} from ${JSON.stringify(text)}`);
    // The expected list: the list, then each ended word, by its first spelling.
    const ended = closed || end ? words : words.slice(0, -1);
    assert.deepEqual(out, unique([...list, ...ended]), JSON.stringify({ list, text, end }));
    assert.equal(rest, closed || end || !words.length ? "" : words.at(-1));
  }
});

test("a paste of the same address two times gives one chip", () => {
  assert.deepEqual(chips([], "a@example.com,a@example.com,A@EXAMPLE.COM ", false), [["a@example.com"], ""]);
});
