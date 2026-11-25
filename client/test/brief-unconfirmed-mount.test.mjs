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
