// Watches in the brief (card R5-07): the one notice of a mail that came, the
// list of the watches that wait, and the field that starts a watch.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const { arrivedLine } = await load("/src/watches.ts");
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
  let changed = 0;
  const view = await mount("/src/views/Watches.svelte", { watches: waiting(), onchange: () => changed++ });
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
    assert.equal(changed, 0);

    input.value = "passport from:consulate";
    input.dispatchEvent(new Event("input"));
    view.flushSync();
    submit.click();
    await until(() => changed);
    assert.deepEqual(asked.at(-1), ["POST", "watches", { text: "passport from:consulate" }]);
    view.flushSync();
    assert.equal(view.target.querySelector("input") === null, true, "the field closes");
  } finally {
    view.destroy();
  }
});

test("Stop removes a watch that waits", async () => {
  const asked = serve(() => [200, { removed: "w-1" }]);
  let changed = 0;
  const view = await mount("/src/views/Watches.svelte", { watches: waiting(), onchange: () => changed++ });
  try {
    view.target.querySelector('button[aria-label="Stop the watch for invoice from:dana"]').click();
    await until(() => changed);
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
  const empty = { ...copy(fx.brief), review: { changesets: 0, actions: 0 }, you_owe: { count: 0, items: [] }, owed_to_you: { count: 0, items: [] }, conversations: { count: 0, items: [] } };
  const alone = await mount("/src/views/Brief.svelte", { brief: empty, status: fx.status, watches: [arrived()], poll: 0 });
  try {
    assert.equal(text(alone.target.querySelector("h1")), "The mail that you watched for came.");
    assert.ok(button(alone, "Watch for a mail"), "the quiet button with no watch that waits");
    assert.doesNotMatch(text(alone.target), /Iris watches for/);
  } finally {
    alone.destroy();
  }
});
