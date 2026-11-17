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
  const NAMES = ["Priya", "Priya Shah", '"Shah, Priya"', '"Sam; ops"', ""];
  for (let n = 0; n < 3000; n++) {
    const list = Array.from({ length: r.below(4) }, () => r.pick(POOL));
    // Each word: an address, bare or with a name in front (as in a pasted list).
    const words = Array.from({ length: r.below(8) }, () => [r.pick(POOL), r.below(3) === 0 ? r.pick(NAMES) : null]);
    // A named entry follows a comma or a semicolon, as in a list of mail
    // headers; a space alone does not end a name.
    const LIST_SEPS = SEPS.filter((x) => /[,;]/.test(x));
    let text = words.map(([a, name], i) => (i ? r.pick(name === null ? SEPS : LIST_SEPS) : "") + (name === null ? a : `${name} <${a}>`)).join("");
    const closed = r.below(2) === 0;
    if (closed && text) text += r.pick(SEPS);
    const end = r.below(3) === 0;
    const [out, rest] = chips(list, text, end);
    const lower = out.map((a) => a.toLowerCase());
    assert.equal(new Set(lower).size, out.length, `duplicate in ${JSON.stringify(out)} from ${JSON.stringify(text)}`);
    // A bare address at the open end stays text; an address in <> is complete.
    const last = words.at(-1);
    const open = !closed && !end && last && last[1] === null;
    const ended = (open ? words.slice(0, -1) : words).map(([a]) => a);
    assert.deepEqual(out, unique([...list, ...ended]), JSON.stringify({ list, text, end }));
    assert.equal(rest, open ? last[0] : "", JSON.stringify({ text, end }));
  }
});

test("a paste of the same address two times gives one chip", () => {
  assert.deepEqual(chips([], "a@example.com,a@example.com,A@EXAMPLE.COM ", false), [["a@example.com"], ""]);
  assert.deepEqual(chips([], '"Shah, Priya" <priya@example.com>, Sam Okafor <sam@okafor.example>'), [["priya@example.com", "sam@okafor.example"], ""]);
});
