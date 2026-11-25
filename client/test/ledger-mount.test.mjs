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
    let answer = answers[`${init?.method ?? "GET"} ${path}`] ?? [404, { error: "Not found." }];
    if (typeof answer === "function") answer = await answer();
    const [status, body] = answer;
    if (status === 0) throw new TypeError("Failed to fetch");
    return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  };
  return { asked, restore: () => (globalThis.fetch = saved) };
}

test("each line says what, why and who, and the undo starts once", async () => {
  const page = fx.ledger;
  const first = page.entries[0];
  const net = serve({ [`POST ledger/${first.id}/undo`]: [200, { ...first, can_undo: false, undo_open: true }] });
  const app = await mount("/src/views/Ledger.svelte", { page });
  try {
    const t = text(app.target);
    assert.match(t, /Archived 18 messages/);
    assert.match(t, /Iris triage\. Rule 5 \(Archive old newsletters\) allows archive\./);
    assert.match(t, /Moved to the trash 2 messages/);
    assert.match(t, /An agent, job “Pay the Acme invoice”, approved by you/);
    assert.match(t, /Failed\. 12 of 40 messages changed\./);
    assert.match(t, /The undo of an earlier change\./);
    assert.match(t, /Closed “Send the Q4 numbers to Priya”/);
    // An undone entry and a state verb have no Undo.
    const buttons = () => [...app.target.querySelectorAll("li button")];
    assert.equal(buttons().length, page.entries.filter((e) => e.can_undo && !e.undo_open).length);
    buttons()[0].click();
    await until(() => text(app.target).includes("Iris undoes the change."));
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
    "GET ledger?limit=200": [200, now],
  });
  const app = await mount("/src/views/Ledger.svelte", { page });
  try {
    app.target.querySelector("li button").click();
    await until(() => text(app.target).includes("Iris did not start the undo: an undo of l-31 is open"));
    app.flushSync();
    // The first line is as the engine says now: no Undo.
    assert.equal(app.target.querySelector("li").querySelector("button") === null, true);
    assert.match(text(app.target), /Rule 5 \(Archive old newsletters\)/);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("with no answer, the view says that it does not know", async () => {
  const page = fx.ledger;
  const first = page.entries[0];
  const net = serve({ [`POST ledger/${first.id}/undo`]: [0, null], "GET ledger?limit=200": [200, page] });
  const app = await mount("/src/views/Ledger.svelte", { page });
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
  const app = await mount("/src/views/Ledger.svelte", { page });
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

// After the start, the view reads the list again until the undo ends: the first entry shows
// "Undone", and the entry of the undo shows at the top. A failed undo gives the Undo back.
for (const [name, end] of [
  ["done", (first) => [{ ...first, state: "undone", can_undo: false, undo_open: false, undone_by: "l-40" }, { id: "l-40", state: "applied", undo_of: first.id }]],
  ["failed", (first) => [{ ...first, can_undo: true, undo_open: false }, { id: "l-40", state: "failed", error: "Gmail refused the call.", undo_of: first.id, changed: 0, messages: 18 }]],
]) {
  test(`the view follows a started undo to its end (${name})`, async () => {
    const page = fx.ledger;
    const first = page.entries[0];
    const [old, undo] = end(first);
    const now = { next: null, entries: [{ ...first, ...undo, verb: "inbox", why: "Undo of l-31", can_undo: false, undone_by: null }, old, ...page.entries.slice(1)] };
    const net = serve({
      [`POST ledger/${first.id}/undo`]: [200, { ...first, can_undo: false, undo_open: true }],
      "GET ledger?limit=200": [200, now],
    });
    const app = await mount("/src/views/Ledger.svelte", { page, wait: 10 });
    try {
      app.target.querySelector("li button").click();
      await until(() => text(app.target).includes("Moved to the inbox 18 messages"));
      app.flushSync();
      const t = text(app.target);
      assert.match(t, /The undo of the change at/);
      assert.doesNotMatch(t, /Iris undoes the change\./);
      if (name === "done") assert.match(t, /Undone/);
      else assert.match(t, /Failed\. 0 of 18 messages changed\. Gmail refused the call\./);
      const undos = [...app.target.querySelectorAll("li")].slice(0, 2).map((li) => li.querySelector("button") !== null);
      assert.deepEqual(undos, [false, name === "failed"]);
      // No read after the end: nothing is open.
      const reads = net.asked.filter((a) => a === "GET ledger?limit=200").length;
      await new Promise((r) => setTimeout(r, 50));
      assert.equal(net.asked.filter((a) => a === "GET ledger?limit=200").length, reads);
    } finally {
      app.destroy();
      net.restore();
    }
  });
}

// The undo line promises no count: the undo skips each message that changed again or is gone
// (specification 2, section 7.4), so `changed` is not the count that the undo puts back.
test("the line of an open undo promises no count of messages", async () => {
  const first = { ...fx.ledger.entries[0], changed: 18, can_undo: false, undo_open: true };
  const app = await mount("/src/views/Ledger.svelte", { page: { next: null, entries: [first] }, wait: 60000 });
  try {
    const line = text(app.target.querySelector(".going"));
    assert.match(line, /Iris undoes the change\. A message that changed again, or that is gone, stays as it is\./);
    assert.doesNotMatch(line, /\d/);
  } finally {
    app.destroy();
  }
});

// The undo of a state entry changes items, not messages: its line says so.
test("the line of an open undo of an item speaks of items", async () => {
  const close = { ...fx.ledger.entries.find((e) => e.verb === "close"), can_undo: false, undo_open: true };
  const app = await mount("/src/views/Ledger.svelte", { page: { next: null, entries: [close] }, wait: 60000 });
  try {
    assert.match(text(app.target.querySelector(".going")), /An item that changed again, or that is gone, stays as it is\./);
  } finally {
    app.destroy();
  }
});

// A read of the list again and Show more never overlap: if they did, the read again (started
// first, ended last) put back the list of before, without the new page, and kept its cursor.
test("a read of the list again does not drop a page that Show more added", async () => {
  const gate = () => {
    let open;
    const p = new Promise((r) => (open = r));
    return { wait: () => p, open };
  };
  const [held, again] = [gate(), gate()];
  const entries = fx.ledger.entries.map((e, i) => (i === 0 ? { ...e, can_undo: false, undo_open: true } : e));
  const extra = { ...fx.ledger.entries.at(-1), id: "l-9", at: "2026-11-01T10:00:00Z" };
  const net = serve({
    "GET ledger?limit=50&after=25": async () => (await held.wait(), [200, { entries: [extra], next: "8" }]),
    "GET ledger?limit=200": async () => (await again.wait(), [200, { entries: [{ ...entries[0], undo_open: false }, ...entries.slice(1), extra], next: "8" }]),
  });
  const app = await mount("/src/views/Ledger.svelte", { page: { entries, next: "25" }, wait: 10 });
  try {
    app.target.querySelector("button.more").click();
    await new Promise((r) => setTimeout(r, 50));
    held.open();
    await until(() => app.target.querySelectorAll("li").length === entries.length + 1);
    again.open();
    await until(() => net.asked.includes("GET ledger?limit=200") && !app.target.querySelector(".going"));
    await new Promise((r) => setTimeout(r, 50));
    app.flushSync();
    assert.equal(app.target.querySelectorAll("li").length, entries.length + 1);
    assert.equal(app.target.querySelector("button.more") !== null, true);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("an entry with messages that Iris could not confirm lists them, a page at a time", async () => {
  const e = { ...fx.ledger.entries[0], id: "l-41", state: "failed", unknown: 2, can_undo: false, undo_open: false };
  const msg = (id, subject) => ({ id, thread: "t-1", from: { name: "Dana", address: "dana@example.com" }, to: [], subject, snippet: "", date: "2026-11-15T09:00:00Z", labels: [], unread: false, gone: false });
  const net = serve({
    "GET ledger/l-41/unknown?limit=50": [200, { messages: [msg("m-7", "Invoice 7")], next: "m-7" }],
    "GET ledger/l-41/unknown?limit=50&after=m-7": [200, { messages: [{ id: "m-8", gone: true }], next: null }],
  });
  const app = await mount("/src/views/Ledger.svelte", { page: { entries: [e], next: null } });
  try {
    const button = () => [...app.target.querySelectorAll("button.check")][0];
    assert.match(text(app.target), /Iris could not confirm 2 messages: check them in your mailbox\./);
    button().click();
    await until(() => app.target.querySelector("ul.unknown a") !== null);
    const link = app.target.querySelector("ul.unknown a");
    assert.match(text(link), /Dana Invoice 7/);
    assert.match(link.getAttribute("href"), /m-7/);
    assert.equal(text(button()), "Show more");
    button().click();
    await until(() => /not in Iris any more/.test(text(app.target)));
    assert.equal(button(), undefined);
    assert.deepEqual(net.asked.filter((k) => k.includes("unknown")), ["GET ledger/l-41/unknown?limit=50", "GET ledger/l-41/unknown?limit=50&after=m-7"]);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a failed read of the messages to check keeps a button to try again", async () => {
  const e = { ...fx.ledger.entries[0], id: "l-42", state: "failed", unknown: 1, can_undo: false, undo_open: false };
  let tries = 0;
  const net = serve({
    // The first read gets no answer, and the second a 500 with words: each keeps the button.
    "GET ledger/l-42/unknown?limit=50": () => (++tries === 1 ? [0, null] : tries === 2 ? [500, { error: "The store is busy." }] : [200, { messages: [{ id: "m-9", gone: true }], next: null }]),
  });
  const app = await mount("/src/views/Ledger.svelte", { page: { entries: [e], next: null } });
  try {
    const button = () => app.target.querySelector("button.check");
    button().click();
    await until(() => button() !== null && text(button()) === "Try again");
    button().click();
    await until(() => /The store is busy\./.test(text(app.target)) && text(button()) === "Try again");
    button().click();
    await until(() => /not in Iris any more/.test(text(app.target)));
    assert.equal(button(), null);
    assert.equal(tries, 3);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a recovery that lists one more message starts the list of messages to check again", async () => {
  const e = { ...fx.ledger.entries[0], id: "l-43", state: "writing", unknown: 1, can_undo: false, undo_open: false };
  const msg = (id, subject) => ({ id, thread: "t-1", from: { name: "Dana", address: "dana@example.com" }, to: [], subject, snippet: "", date: "2026-11-15T09:00:00Z", labels: [], unread: false, gone: false });
  let listed = [msg("m-5", "Invoice 5")];
  let entry = e;
  const net = serve({
    "GET ledger/l-43/unknown?limit=50": () => [200, { messages: listed, next: null }],
    "GET ledger?limit=200": () => [200, { entries: [entry], next: null }],
  });
  const app = await mount("/src/views/Ledger.svelte", { page: { entries: [e], next: null }, wait: 20 });
  try {
    const button = () => app.target.querySelector("button.check");
    button().click();
    await until(() => button() === null && /Invoice 5/.test(text(app.target)));
    // The recovery lists a message with an earlier ID, and the next read of the list shows the new count.
    listed = [msg("m-2", "Invoice 2"), msg("m-5", "Invoice 5")];
    entry = { ...e, unknown: 2 };
    await until(() => button() !== null);
    button().click();
    await until(() => /Invoice 2/.test(text(app.target)));
    assert.equal(app.target.querySelectorAll("ul.unknown a").length, 2);
  } finally {
    app.destroy();
    net.restore();
  }
});
