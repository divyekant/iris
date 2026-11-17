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

test("the cards follow the engine: a cancel note stays, and a new send of the draft shows", async () => {
  const all = fx.outbox().sends;
  let sends = all;
  const net = serve((path, method) => {
    if (method === "POST" && path === "outbox/d-14/cancel") {
      sends = sends.filter((x) => x.draft !== "d-14");
      return [200, { cancelled: "d-14" }];
    }
    if (method === "POST" && path === "outbox/d-15/cancel") {
      // The release claimed d-15 and it leaves: the next read has no d-15.
      sends = sends.filter((x) => x.draft !== "d-15");
      return [409, { error: "The send started. It cannot be cancelled." }];
    }
    return [200, { ...fx.outbox(), count: sends.length, sends }];
  });
  const app = await mount("/src/views/Outbox.svelte", { first: fx.outbox() });
  const cards = () => [...app.target.querySelectorAll("li.card")];
  const button = (subject) => cards().find((c) => c.textContent.includes(subject)).querySelector("button");
  try {
    button("Signing").click();
    await until(() => cards().length === 2);
    app.flushSync();
    assert.match(text(app.target.querySelector(".notes")), /"Re: Signing on Friday" is cancelled\. It does not leave\./);
    button("Renewal").click();
    await until(() => cards().length === 1);
    app.flushSync();
    // The card went with the poll, and the note says why.
    assert.match(text(app.target.querySelector(".notes")), /"Renewal: \$48,000 for 12 months" started to send\. It cannot be cancelled\./);
    // A new send of the cancelled draft d-14 comes back from the engine: it shows, with its Cancel button.
    sends = [...sends, { ...all[0], subject: "Re: Signing on Friday, again" }];
    app.target.querySelector("li.card button").click(); // cancel d-16; the reload brings the new d-14
    await until(() => cards().some((c) => c.textContent.includes("again")));
    app.flushSync();
    assert.ok(button("again"));
    assert.ok(net.asked.includes("POST outbox/d-14/cancel"));
  } finally {
    app.destroy();
    net.restore();
  }
});

test("the fixture server cancels a send, as the API does", () => {
  const before = fx.outbox().sends.length;
  assert.deepEqual(fx.outboxPost("/outbox/d-16/cancel"), [200, { cancelled: "d-16" }]);
  assert.equal(fx.outbox().sends.length, before - 1);
  assert.equal(fx.outbox().count, before - 1);
  assert.equal(fx.outboxPost("/outbox/d-16/cancel")[0], 404);
  assert.equal(fx.outboxPost("/outbox/d-16/other"), undefined);
});
