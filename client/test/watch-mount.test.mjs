// Watches in the brief (card R5-07): the one notice of a mail that came, the
// list of the watches that wait, and the field that starts a watch.
// A zone west of UTC: there, a UTC date-time after midnight is still the day before.
process.env.TZ = "America/Los_Angeles";
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const { arrivedLine, day } = await load("/src/watches.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");
const copy = (x) => JSON.parse(JSON.stringify(x));
const arrived = () => copy(fx.watches.find((w) => w.state === "arrived"));
const waiting = () => copy(fx.watches.filter((w) => w.state === "waits"));

// Answer each request with `answer(path, body)`: [status, body].
function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    const body = init?.body ? JSON.parse(init.body) : undefined;
    asked.push([init?.method ?? "GET", path, body]);
    const [status, out] = await answer(path, body);
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

const button = (view, name) => [...view.target.querySelectorAll("button")].find((b) => text(b).trim().startsWith(name));

test("the opening sentence counts the mails that came", () => {
  assert.equal(arrivedLine([]), "");
  assert.equal(arrivedLine([arrived()]), "The mail that you watched for came.");
  assert.equal(arrivedLine([arrived(), arrived()]), "Two mails that you watched for came.");
});

test("the notice opens the mail, and Done removes the watch", async () => {
  const asked = serve(() => [200, { removed: "w-3" }]);
  let gone = null;
  const view = await mount("/src/views/Arrived.svelte", { watch: arrived(), ondone: (id) => (gone = id) });
  try {
    assert.match(text(view.target), /You watched for “?visa from:consulate/);
    const link = view.target.querySelector("a");
    assert.equal(link.getAttribute("href"), "#/mail/m-90");
    assert.equal(text(link), "Your visa application: decision");
    button(view, "Done").click();
    await until(() => gone);
    assert.equal(gone, "w-3");
    assert.deepEqual(asked, [["POST", "watches/w-3/remove", {}]]);
  } finally {
    view.destroy();
  }
});

test("Done keeps the notice and shows the reason when the engine refuses", async () => {
  serve(() => [500, { error: "database: locked" }]);
  let gone = null;
  const view = await mount("/src/views/Arrived.svelte", { watch: arrived(), ondone: (id) => (gone = id) });
  try {
    button(view, "Done").click();
    await until(() => view.target.querySelector("[role=alert]"));
    assert.match(text(view.target), /database: locked/);
    assert.equal(gone, null);
    // A watch that is gone already (another tab) leaves the brief.
    serve(() => [404, { error: "not found" }]);
    button(view, "Done").click();
    await until(() => gone);
  } finally {
    view.destroy();
  }
});

test("a mail that left the mailbox shows as words, with no link", async () => {
  const watch = { ...arrived(), message: null };
  const view = await mount("/src/views/Arrived.svelte", { watch, ondone: () => {} });
  try {
    assert.match(text(view.target), /The mail came on .*, then it left your mailbox\./);
    assert.equal(view.target.querySelector("a") === null, true);
  } finally {
    view.destroy();
  }
});

test("a new watch starts from the quiet button, and the engine's reason shows", async () => {
  const asked = serve((path, body) => (body.text.includes("form:email") ? [400, { error: "form: takes one of commitment, conversation" }] : [201, { id: "w-9" }]));
  let added = null;
  const view = await mount("/src/views/Watches.svelte", { watches: waiting(), onadded: (w) => (added = w), onstopped: () => {} });
  try {
    assert.match(text(view.target), /"lease renewal" from:harbor/);
    button(view, "Watch for another mail").click();
    view.flushSync();
    const input = view.target.querySelector("input");
    const submit = button(view, "Watch");
    assert.equal(submit.disabled, true, "no empty watch");

    input.value = "form:email";
    input.dispatchEvent(new Event("input"));
    view.flushSync();
    submit.click();
    await until(() => view.target.querySelector("[role=alert]"));
    assert.match(text(view.target), /form: takes one of/);
    assert.equal(added, null);

    input.value = "passport from:consulate";
    input.dispatchEvent(new Event("input"));
    view.flushSync();
    submit.click();
    await until(() => added);
    assert.equal(added.id, "w-9");
    assert.deepEqual(asked.at(-1), ["POST", "watches", { text: "passport from:consulate" }]);
    view.flushSync();
    assert.equal(view.target.querySelector("input") === null, true, "the field closes");
  } finally {
    view.destroy();
  }
});

test("Stop removes a watch that waits", async () => {
  const asked = serve(() => [200, { removed: "w-1" }]);
  let stopped = null;
  const view = await mount("/src/views/Watches.svelte", { watches: waiting(), onadded: () => {}, onstopped: (id) => (stopped = id) });
  try {
    view.target.querySelector('button[aria-label="Stop the watch for invoice from:dana"]').click();
    await until(() => stopped);
    assert.equal(stopped, "w-1");
    assert.deepEqual(asked, [["POST", "watches/w-1/remove", {}]]);
  } finally {
    view.destroy();
  }
});

test("the brief leads with the mail that came, and lists the watches that wait", async () => {
  serve(() => [200, {}]);
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, status: fx.status, watches: copy(fx.watches), poll: 0 });
  try {
    const h1 = text(view.target.querySelector("h1"));
    assert.ok(h1.startsWith("The mail that you watched for came. Four changes wait"), h1);
    assert.match(text(view.target), /Iris watches for 2/);
  } finally {
    view.destroy();
  }
  const empty = { ...copy(fx.brief), review: { changesets: 0, actions: 0 }, you_owe: { count: 0, items: [] }, owed_to_you: { count: 0, items: [] }, conversations: { count: 0, items: [] }, strangers: { count: 0, items: [] }, gate: { count: 0, senders: [], next: null } };
  const alone = await mount("/src/views/Brief.svelte", { brief: empty, status: fx.status, watches: [arrived()], poll: 0 });
  try {
    assert.equal(text(alone.target.querySelector("h1")), "The mail that you watched for came.");
    assert.ok(button(alone, "Watch for a mail"), "the quiet button with no watch that waits");
    assert.doesNotMatch(text(alone.target), /Iris watches for/);
  } finally {
    alone.destroy();
  }
});

test("the start day of a watch is the local day of the viewer", () => {
  assert.equal(day("2026-11-16T00:30:00Z"), new Date(2026, 10, 15).toLocaleDateString(undefined, { dateStyle: "medium" }));
  assert.equal(day("not a date"), "not a date");
});

test("a failed watch load keeps the last watches and does not stop the brief", async () => {
  const changed = { ...copy(fx.brief), review: { changesets: 0, actions: 0 } };
  serve((path) => (path === "watches" ? [500, { error: "database: locked" }] : path === "status" ? [200, fx.status] : path.startsWith("brief/signals") ? [200, { items: [], next: null }] : [200, changed]));
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, status: fx.status, watches: copy(fx.watches), poll: 5 });
  try {
    await until(() => !text(view.target.querySelector("h1")).includes("for your review"));
    assert.match(text(view.target.querySelector("h1")), /^The mail that you watched for came\. You owe/);
    // Wait for more than one failed watch load: the list stays.
    await new Promise((r) => setTimeout(r, 150));
    assert.match(text(view.target), /Iris watches for 2/);
  } finally {
    view.destroy();
  }
});

test("the brief shows at once while the watches still load", async () => {
  serve((path) => (path === "watches" ? new Promise(() => {}) : [200, {}]));
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, status: fx.status, poll: 0 });
  try {
    assert.match(text(view.target.querySelector("h1")), /^Four changes wait/);
    assert.ok(button(view, "Watch for a mail"));
  } finally {
    view.destroy();
  }
});

test("an older watch answer never undoes a newer change, and a change shows without a reload", async () => {
  // Each GET of the watches waits until the test lets it go, with the list of that moment.
  const gets = [];
  serve((path, body) => {
    if (path === "watches" && body === undefined) return new Promise((r) => gets.push(() => r([200, copy(fx.watches)])));
    if (path === "watches") return [201, { id: "w-20", text: body.text, created: "2026-11-16T10:00:00Z", state: "waits", arrived: null, message: null }];
    return [200, { removed: path.split("/")[1] }];
  });
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, status: fx.status, watches: copy(fx.watches), poll: 5 });
  try {
    await until(() => gets.length);
    button(view, "Done").click();
    await until(() => !view.target.querySelector("article"));
    gets.splice(0).forEach((go) => go()); // The old answers still hold w-3.
    await new Promise((r) => setTimeout(r, 30));
    assert.equal(view.target.querySelector("article") === null, true, "the notice stays gone");

    // An add shows at once, also while the watch loads fail.
    serve((path, body) => (path === "watches" && body === undefined ? [500, { error: "locked" }] : [201, { id: "w-20", text: body.text, created: "2026-11-16T10:00:00Z", state: "waits", arrived: null, message: null }]));
    button(view, "Watch for another mail").click();
    view.flushSync();
    const input = view.target.querySelector("input");
    input.value = "passport";
    input.dispatchEvent(new Event("input"));
    view.flushSync();
    button(view, "Watch").click();
    await until(() => /Iris watches for 3/.test(text(view.target)));
    await new Promise((r) => setTimeout(r, 150));
    assert.match(text(view.target), /Iris watches for 3/);
  } finally {
    view.destroy();
  }
});

test("the fixture gives each new watch its own ID", () => {
  const [, a] = fx.watchPost("/watches", { text: "one" });
  fx.watchPost(`/watches/${a.id}/remove`, {});
  fx.watchPost("/watches/w-1/remove", {});
  const [, b] = fx.watchPost("/watches", { text: "two" });
  const [, c] = fx.watchPost("/watches", { text: "three" });
  assert.equal(new Set([a.id, b.id, c.id]).size, 3);
});
