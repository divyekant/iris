// The outbox (R3-02) in a DOM: each send that waits, its warnings and time
// left, the banner while sending is off, and the cancel of the person.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const api = await load("/src/api.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");

function serve(answer) {
  const asked = [];
  const saved = globalThis.fetch;
  globalThis.fetch = async (url, init = {}) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push(`${init.method ?? "GET"} ${path}`);
    const [status, body] = answer(path, init.method ?? "GET");
    return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  };
  return { asked, restore: () => (globalThis.fetch = saved) };
}

test("the outbox reads each page, so no send that waits is hidden", async () => {
  const all = fx.outbox();
  const pages = {
    outbox: { ...all, sends: all.sends.slice(0, 1), next: "5" },
    "outbox?after=5": { ...all, sends: all.sends.slice(1, 2), next: "6" },
    "outbox?after=6": { ...all, sends: all.sends.slice(2), next: null },
  };
  const net = serve((path) => (pages[path] ? [200, pages[path]] : [404, { error: "Not found." }]));
  try {
    const box = await api.loadOutbox();
    assert.deepEqual(box.sends.map((s) => s.draft), ["d-14", "d-15", "d-16"]);
    assert.equal(box.next, null);
    assert.deepEqual(net.asked, ["GET outbox", "GET outbox?after=5", "GET outbox?after=6"]);
  } finally {
    net.restore();
  }
});

test("each card shows the recipients, the warnings and the time left", async () => {
  const net = serve(() => [200, fx.outbox()]);
  const app = await mount("/src/views/Outbox.svelte", { first: fx.outbox() });
  try {
    const cards = [...app.target.querySelectorAll("li.card")];
    assert.equal(cards.length, 3);
    assert.match(text(cards[1]), /To dana@acme\.example, legal@new\.example/);
    assert.match(text(cards[1]), /Goes to an address you never wrote to/);
    assert.match(text(cards[1]), /Names an amount of money/);
    assert.match(text(cards[0]), /Leaves in 0:[34]\d/);
    assert.equal(app.target.querySelector(".off"), null);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("while sending is off, a banner says so and no send has a time", async () => {
  const off = { ...fx.outbox(), sending: { on: false, since: null } };
  off.sends = off.sends.map((s) => ({ ...s, leaves: null }));
  const net = serve(() => [200, off]);
  const app = await mount("/src/views/Outbox.svelte", { first: off });
  try {
    assert.match(text(app.target.querySelector(".off")), /Sending is off\. Sends wait here\./);
    assert.match(text(app.target.querySelector(".off")), /iris sending on/);
    assert.equal(app.target.querySelectorAll(".bar").length, 0);
    assert.match(text(app.target), /Waits until sending is on/);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a cancel removes the card; a send that started keeps its card and says why", async () => {
  const net = serve((path, method) => {
    if (method === "POST" && path === "outbox/d-14/cancel") return [200, { cancelled: "d-14" }];
    if (method === "POST" && path === "outbox/d-15/cancel") return [409, { error: "The send started. It cannot be cancelled." }];
    return [200, fx.outbox()];
  });
  const app = await mount("/src/views/Outbox.svelte", { first: fx.outbox() });
  try {
    const cancel = (draft) => [...app.target.querySelectorAll("li.card")].find((c) => c.textContent.includes(draft === "d-14" ? "Signing" : "Renewal")).querySelector("button");
    cancel("d-14").click();
    await until(() => app.target.querySelectorAll("li.card").length === 2);
    app.flushSync();
    assert.match(text(app.target), /Cancelled\. It does not leave\./);
    cancel("d-15").click();
    await until(() => app.target.querySelector('[role="alert"]'));
    assert.match(text(app.target.querySelector('[role="alert"]')), /The send started\. It cannot be cancelled\./);
    assert.equal(app.target.querySelectorAll("li.card").length, 2);
    assert.ok(net.asked.includes("POST outbox/d-14/cancel"));
  } finally {
    app.destroy();
    net.restore();
  }
});
