// The history view (R2-07) in a DOM: the lines, the undo, and the next page.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");

function serve(answers) {
  const asked = [];
  const saved = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push(`${init?.method ?? "GET"} ${path}`);
    const [status, body] = answers[`${init?.method ?? "GET"} ${path}`] ?? [404, { error: "Not found." }];
    if (status === 0) throw new TypeError("Failed to fetch");
    return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  };
  return { asked, restore: () => (globalThis.fetch = saved) };
}

test("each line says what, why and who, and the undo starts once", async () => {
  const page = fx.ledger;
  const first = page.entries[0];
  const net = serve({ [`POST ledger/${first.id}/undo`]: [200, { ...first, can_undo: false, undo_open: true }] });
  const app = await mount("/src/views/Ledger.svelte", { page, rules: { 5: "Archive old newsletters" } });
  try {
    const t = text(app.target);
    assert.match(t, /Archived 18 messages/);
    assert.match(t, /Iris triage, allowed by your rule “Archive old newsletters”/);
    assert.match(t, /Moved to the trash 2 messages/);
    assert.match(t, /An agent, job “Pay the Acme invoice”, approved by you/);
    assert.match(t, /Failed\. 12 of 40 messages changed\./);
    assert.match(t, /The undo of an earlier change\./);
    assert.match(t, /Closed “Send the Q4 numbers to Priya”/);
    // An undone entry and a state verb have no Undo.
    const buttons = () => [...app.target.querySelectorAll("li button")];
    assert.equal(buttons().length, page.entries.filter((e) => e.can_undo && !e.undo_open).length);
    buttons()[0].click();
    await until(() => text(app.target).includes("Iris puts 18 messages back"));
    app.flushSync();
    assert.equal(buttons().length, page.entries.filter((e) => e.can_undo && !e.undo_open).length - 1);
    assert.deepEqual(net.asked, [`POST ledger/${first.id}/undo`]);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a refused undo shows the reason and loads the list again", async () => {
  const page = fx.ledger;
  const first = page.entries[0];
  const now = { ...page, entries: [{ ...first, can_undo: false }, ...page.entries.slice(1)] };
  const net = serve({
    [`POST ledger/${first.id}/undo`]: [409, { error: "an undo of l-31 is open" }],
    "GET ledger?limit=50": [200, now],
  });
  const app = await mount("/src/views/Ledger.svelte", { page, rules: {} });
  try {
    app.target.querySelector("li button").click();
    await until(() => text(app.target).includes("Iris did not start the undo: an undo of l-31 is open"));
    app.flushSync();
    // The first line is as the engine says now: no Undo.
    assert.equal(app.target.querySelector("li").querySelector("button") === null, true);
    assert.match(text(app.target), /your rule 5/);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("with no answer, the view says that it does not know", async () => {
  const page = fx.ledger;
  const first = page.entries[0];
  const net = serve({ [`POST ledger/${first.id}/undo`]: [0, null], "GET ledger?limit=50": [200, page] });
  const app = await mount("/src/views/Ledger.svelte", { page, rules: {} });
  try {
    app.target.querySelector("li button").click();
    await until(() => text(app.target).includes("Iris does not know if the undo started."));
  } finally {
    app.destroy();
    net.restore();
  }
});

test("the next page adds each entry once", async () => {
  const page = { ...fx.ledger, next: "30" };
  const extra = { ...fx.ledger.entries.at(-1), id: "l-9", at: "2026-11-01T10:00:00Z" };
  const net = serve({ "GET ledger?limit=50&after=30": [200, { entries: [page.entries.at(-1), extra], next: null }] });
  const app = await mount("/src/views/Ledger.svelte", { page, rules: {} });
  try {
    app.target.querySelector("button.more").click();
    await until(() => app.target.querySelectorAll("li").length === page.entries.length + 1);
    app.flushSync();
    assert.equal(app.target.querySelector("button.more") === null, true);
  } finally {
    app.destroy();
    net.restore();
  }
});
