// Spec 2 section 7.3 (issue 216): the brief shows the changes that Iris could not confirm in
// one line, until you dismiss it. The dismissal names the entries that the line showed.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");
const NOW = new Date("2026-11-16T09:05:00Z");

function serve(answers) {
  const asked = [];
  const saved = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    const key = `${init?.method ?? "GET"} ${path}`;
    asked.push({ key, body: init?.body ?? null });
    const [status, body] = answers[key] ?? [404, { error: "Not found." }];
    return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  };
  return { asked, restore: () => (globalThis.fetch = saved) };
}

test("the line shows until you dismiss it, and the dismissal names the entries it showed", async () => {
  const brief = { ...fx.brief, unconfirmed: { messages: 3, entries: ["l-9", "l-7"] } };
  const net = serve({ "POST ledger/unconfirmed/dismiss": [200, { dismissed: 2 }] });
  const app = await mount("/src/views/Brief.svelte", { brief, now: NOW, poll: 0 });
  try {
    const line = () => app.target.querySelector("p.unconfirmed");
    assert.match(text(line()), /Iris could not confirm a change to 3 messages\. Check them in your mailbox\./);
    assert.equal(line().querySelector("a").getAttribute("href"), "#/history");
    line().querySelector("button").click();
    await until(() => line() === null);
    const posted = net.asked.find((a) => a.key === "POST ledger/unconfirmed/dismiss");
    assert.deepEqual(JSON.parse(posted.body), { entries: ["l-9", "l-7"] });
  } finally {
    app.destroy();
    net.restore();
  }
});

test("with more entries than the line listed, the dismissal shows the next line", async () => {
  const brief = { ...fx.brief, unconfirmed: { messages: 2, entries: ["l-9"], more: 1 } };
  const next = { ...fx.brief, unconfirmed: { messages: 1, entries: ["l-4"], more: 0 } };
  const net = serve({ "POST ledger/unconfirmed/dismiss": [200, { dismissed: 1 }], "GET brief": [200, next] });
  const app = await mount("/src/views/Brief.svelte", { brief, now: NOW, poll: 0 });
  try {
    const line = () => app.target.querySelector("p.unconfirmed");
    line().querySelector("button").click();
    await until(() => line() !== null && /1 message\./.test(text(line())));
    assert.ok(net.asked.some((a) => a.key === "GET brief"));
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a newer line that a poll shows during the dismissal stays", async () => {
  const brief = { ...fx.brief, unconfirmed: { messages: 1, entries: ["l-9"], more: 0 } };
  const newer = { ...fx.brief, unconfirmed: { messages: 2, entries: ["l-12", "l-9"], more: 0 } };
  let polled = () => {};
  const seen = new Promise((r) => (polled = r));
  let briefs = 0;
  const saved = globalThis.fetch;
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const key = `${init?.method ?? "GET"} ${String(url).replace(/^\/api\//, "")}`;
    asked.push(key);
    let answer = [404, { error: "Not found." }];
    if (key === "GET status") answer = [200, fx.status];
    // The first poll shows the newer line. Later reads fail, so only the view keeps it.
    if (key === "GET brief" && ++briefs === 1) answer = [200, newer];
    if (key === "POST ledger/unconfirmed/dismiss") {
      // Answer after the poll put the newer line on the screen.
      await seen;
      await new Promise((r) => setTimeout(r, 30));
      answer = [200, { dismissed: 1 }];
    }
    if (key === "GET brief" && briefs === 1) polled();
    return new Response(JSON.stringify(answer[1]), { status: answer[0], headers: { "content-type": "application/json" } });
  };
  const app = await mount("/src/views/Brief.svelte", { brief, now: NOW, poll: 20 });
  try {
    const line = () => app.target.querySelector("p.unconfirmed");
    line().querySelector("button").click();
    await until(() => /2 messages\./.test(text(line() ?? app.target)));
    await until(() => asked.filter((k) => k === "GET brief").length >= 2);
    await new Promise((r) => setTimeout(r, 30));
    assert.notEqual(line(), null);
    assert.match(text(line()), /2 messages\./);
  } finally {
    app.destroy();
    globalThis.fetch = saved;
  }
});

test("a refused dismissal keeps the line and says why", async () => {
  const brief = { ...fx.brief, unconfirmed: { messages: 1, entries: ["l-9"] } };
  const net = serve({ "POST ledger/unconfirmed/dismiss": [400, { error: "no such entry form" }] });
  const app = await mount("/src/views/Brief.svelte", { brief, now: NOW, poll: 0 });
  try {
    assert.match(text(app.target), /Iris could not confirm a change to 1 message\. Check it in your mailbox\./);
    app.target.querySelector("p.unconfirmed button").click();
    await until(() => text(app.target).includes("no such entry form"));
    assert.notEqual(app.target.querySelector("p.unconfirmed"), null);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("no line when nothing waits", async () => {
  const net = serve({});
  const app = await mount("/src/views/Brief.svelte", { brief: { ...fx.brief, unconfirmed: { messages: 0, entries: [] } }, now: NOW, poll: 0 });
  try {
    assert.equal(app.target.querySelector("p.unconfirmed"), null);
  } finally {
    app.destroy();
    net.restore();
  }
});
