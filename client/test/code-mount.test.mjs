// A live login code in the brief (issue #527): Dismiss holds the verb with Undo,
// then posts it and calls ondone. Undo sends nothing.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mount, until } from "./mount.mjs";
import signal from "../../docs/spec/schema/examples/signal-code.json" with { type: "json" };

const now = new Date("2026-11-16T09:05:00Z");
const text = (el) => el.textContent.replace(/\s+/g, " ");
const button = (view, label) => [...view.target.querySelectorAll("button")].find((b) => b.textContent.trim() === label);

function serve(status = 200) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    asked.push([String(url), init?.method]);
    return new Response(JSON.stringify(status === 200 ? {} : { error: "The item changed." }), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

test("Dismiss on a code holds for the Undo time, then posts dismiss and calls ondone", async () => {
  const asked = serve();
  const done = [];
  const view = await mount("/src/views/Code.svelte", { item: signal, now, ondone: (id) => done.push(id), delay: 30 });
  try {
    button(view, "Dismiss").click();
    view.flushSync();
    assert.match(text(view.target), /Dismissed\. It leaves your brief\./);
    assert.ok(button(view, "Undo"));
    assert.deepEqual(asked, [], "nothing leaves during the hold");
    await until(() => done.length === 1);
    assert.deepEqual(asked, [["/api/items/s-901/dismiss", "POST"]]);
    assert.deepEqual(done, ["s-901"]);
  } finally {
    view.destroy();
  }
});

test("Undo of a code Dismiss sends nothing and shows the code again", async () => {
  const asked = serve();
  const view = await mount("/src/views/Code.svelte", { item: signal, now, ondone: () => {}, delay: 30 });
  try {
    button(view, "Dismiss").click();
    view.flushSync();
    button(view, "Undo").click();
    view.flushSync();
    await new Promise((r) => setTimeout(r, 60));
    assert.deepEqual(asked, []);
    assert.match(text(view.target), /482913/);
  } finally {
    view.destroy();
  }
});

test("a refused Dismiss shows the reason; with no ondone, a code has no Dismiss", async () => {
  serve(409);
  const done = [];
  const view = await mount("/src/views/Code.svelte", { item: signal, now, ondone: (id) => done.push(id), delay: 10 });
  try {
    button(view, "Dismiss").click();
    await until(() => /Iris did not record it/.test(text(view.target)));
    assert.deepEqual(done, ["s-901"], "a 409 loads the brief again");
  } finally {
    view.destroy();
  }
  const plain = await mount("/src/views/Code.svelte", { item: signal, now });
  try {
    assert.equal(button(plain, "Dismiss"), undefined);
  } finally {
    plain.destroy();
  }
});

test("in the brief, a code that expires during the Undo time keeps its held card, its Undo and its request", async () => {
  const { load } = await import("./mount.mjs");
  const fx = await load("/dev/fixtures.ts");
  const asked = serve();
  const code = { ...signal, expires: new Date(Date.now() + 300).toISOString() };
  const brief = { ...fx.brief, signals: { count: 1, items: [code] } };
  const view = await mount("/src/views/Brief.svelte", { watches: [], proposed: null, questions: [], brief, now: new Date(), poll: 0, hold: 1200 });
  try {
    button(view, "Dismiss").click();
    view.flushSync();
    await new Promise((r) => setTimeout(r, 600)); // The code expired.
    view.flushSync();
    assert.ok(button(view, "Undo"), "the held card and its Undo stay after the expiry");
    await until(() => asked.some(([url]) => url === "/api/items/s-901/dismiss"));
    await until(() => !/Dismissed\. It leaves your brief\./.test(text(view.target)));
  } finally {
    view.destroy();
  }
});
