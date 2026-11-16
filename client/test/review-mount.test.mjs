// The review screen in a DOM: its effects load the target rows, and its buttons work.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const act = (a) => ({ n: 1, revision: 1, verb: "archive", targets: {}, resolved: { messages: [] }, args: {}, why: null, flags: [], hash: "h", decision: { state: "in_review" }, ...a });
const changeset = (actions) => ({ model: 1, id: "cs-9", revision: 1, author: { kind: "agent", name: "Claude Code" }, why: "Tidy the inbox.", created: "2026-11-15T08:00:00Z", submitted: "2026-11-15T08:00:00Z", state: "open", actions });
const text = (el) => el.textContent.replace(/\s+/g, " ");

// Answer each API call from `routes` (status and body), else from the fixtures. Keep a list of the paths.
function serve(routes = {}) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push(init?.body ? [path, JSON.parse(init.body)] : path);
    const route = routes[path];
    // A function route can wait, throw (no answer) or answer with the request body.
    const [status, body] = typeof route === "function" ? await route(init?.body ? JSON.parse(init.body) : undefined) : (route ?? [200, fx.answer(`/${path}`)]);
    return new Response(JSON.stringify(body ?? { error: "Not found." }), { status: body === undefined ? 404 : status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

test("a request shows every row of each change, with a gone message", async () => {
  const ids = ["m-50140", "m-1", "m-2", "m-3", "m-4"];
  const asked = serve();
  const view = await mount("/src/views/Request.svelte", { changeset: changeset([act({ resolved: { messages: ids } })]) });
  try {
    const rows = () => view.target.querySelectorAll(".messages li");
    await until(() => rows().length === 5);
    assert.match(text(rows()[0]), /not in your mailbox any more/);
    assert.ok(rows()[1].querySelector("a[href]"), "a message row links to the message");
    // One page of up to 200 IDs, and no row behind a button.
    assert.deepEqual(asked.filter((p) => typeof p === "string" && p.startsWith("messages?")), ["messages?ids=m-50140,m-1,m-2,m-3,m-4"]);
    assert.equal(view.target.querySelector("button.more"), null);
  } finally {
    view.destroy();
  }
});

test("only a 404 makes an item gone; another failure shows as a problem", async () => {
  const known = fx.items[0].id;
  serve({ "items/c-gone": [404, { error: "Not found." }], "items/c-down": [500, { error: "The store is busy." }] });
  const gone = await mount("/src/views/Request.svelte", { changeset: changeset([act({ verb: "close", targets: { items: [known, "c-gone"] } })]) });
  try {
    await until(() => gone.target.querySelectorAll(".messages li").length === 2);
    assert.match(text(gone.target), /c-gone: not in Iris any more/);
    assert.equal(gone.target.querySelector("[role=alert]"), null);
  } finally {
    gone.destroy();
  }

  const down = await mount("/src/views/Request.svelte", { changeset: changeset([act({ verb: "close", targets: { items: [known, "c-down"] } })]) });
  try {
    const alert = await until(() => down.target.querySelector("[role=alert]"));
    assert.match(alert.textContent, /^The store is busy\. Try again$/);
    assert.doesNotMatch(text(down.target), /not in Iris any more/);
  } finally {
    down.destroy();
  }
});

test("the review shows an open request whose actions all have a decision, with the decision", async () => {
  serve();
  const done = changeset([act({ resolved: { messages: ["m-1"] }, decision: { state: "approved", by: "person", at: "2026-11-15T09:00:00Z" } })]);
  const view = await mount("/src/views/Review.svelte", { changesets: [done] });
  try {
    const page = text(view.target);
    assert.match(page, /Nothing waits for your review\./);
    assert.doesNotMatch(page, /When an agent or Iris asks/);
    assert.match(page, /Claude Code/);
    assert.match(page, /Tidy the inbox\./);
    const details = view.target.querySelector("details");
    assert.ok(details.open, "the decided changes show open when nothing in the request waits");
    assert.match(text(details), /Archive 1 message/i);
    assert.equal(view.target.querySelector(".decide"), null);
  } finally {
    view.destroy();
  }
});

const three = () =>
  changeset([
    act({ n: 1, hash: "h1", resolved: { messages: ["m-1"] } }),
    act({ n: 2, hash: "h2", verb: "label", args: { label: "Finance" }, resolved: { messages: ["m-2"] } }),
    act({ n: 3, hash: "h3", verb: "read", resolved: { messages: ["m-3"] } }),
  ]);

test("each change starts ticked; the button approves the ticked changes and denies the others", async () => {
  const before = three();
  const asked = serve({
    "changesets/cs-9/decide": (body) => [
      200,
      { ...before, state: "closed", actions: before.actions.map((a) => ({ ...a, decision: body.actions.find((d) => d.n === a.n).approve ? { state: "approved", by: "person" } : { state: "denied", by: "person", reason: "The person denied it in the review." } })) },
    ],
  });
  const view = await mount("/src/views/Review.svelte", { changesets: [before] });
  try {
    const button = () => view.target.querySelector("button.primary");
    const boxes = () => [...view.target.querySelectorAll("input[type=checkbox]")];
    assert.match(text(view.target), /Claude Code asks for 3 changes\./);
    await until(() => !button().disabled);
    assert.ok(boxes().every((b) => b.checked));
    assert.equal(button().textContent, "Approve all 3");

    // A click on the words of a change unticks it too: the label belongs to the box.
    view.target.querySelectorAll("label.what")[1].click();
    view.flushSync();
    assert.equal(boxes()[1].checked, false);
    assert.equal(button().textContent, "Approve 2, deny 1");
    assert.match(text(view.target), /Iris will deny this change\./);

    button().click();
    await until(() => view.target.querySelector("[role=status]"));
    const [, body] = asked.find((x) => Array.isArray(x));
    assert.deepEqual(body.actions, [
      { revision: 1, n: 1, approve: true, hash: "h1" },
      { revision: 1, n: 2, approve: false, hash: "h2" },
      { revision: 1, n: 3, approve: true, hash: "h3" },
    ]);
    view.flushSync();
    const page = text(view.target);
    assert.match(page, /You approved 2 changes\. You denied one change\./);
    assert.match(page, /Nothing waits for your review\./);
    assert.equal(button(), null);
    assert.match(text(view.target.querySelector("details")), /You approved it/);
  } finally {
    view.destroy();
  }
});

test("all unticked denies all; a request that changed saves nothing and shows the request as it is now", async () => {
  const before = three();
  const after = { ...before, revision: 2, why: "Tidy the inbox, without the labels.", actions: [...before.actions.map((a) => ({ ...a, decision: { state: "replaced", by: "author" } })), act({ n: 1, revision: 2, hash: "h4", resolved: { messages: ["m-1"] } })] };
  serve({ "changesets/cs-9/decide": () => [409, { error: "the hash changed" }], "changesets/cs-9": [200, after] });
  const view = await mount("/src/views/Review.svelte", { changesets: [before] });
  try {
    await until(() => !view.target.querySelector("button.primary").disabled);
    for (const box of view.target.querySelectorAll("input[type=checkbox]")) box.click();
    view.flushSync();
    assert.equal(view.target.querySelector("button.primary").textContent, "Deny all 3");
    view.target.querySelector("button.primary").click();
    const alert = await until(() => view.target.querySelector("[role=alert]"));
    assert.match(alert.textContent, /Iris did not save this decision, because the request changed after you opened it\./);
    await until(() => text(view.target).includes("without the labels"));
    view.flushSync();
    assert.match(text(view.target), /Claude Code asks for one change\./);
    assert.equal(view.target.querySelector("button.primary").textContent, "Approve the change");
    assert.equal(view.target.querySelector("[role=status]"), null);
  } finally {
    view.destroy();
  }
});

test("the button waits for the rows of each change, and a failed lookup has a retry", async () => {
  let messages = () => new Promise(() => {});
  serve({ "messages?ids=m-1": () => messages() });
  const one = changeset([act({ resolved: { messages: ["m-1"] } })]);
  const pending = await mount("/src/views/Request.svelte", { changeset: one });
  try {
    await new Promise((r) => setTimeout(r, 50));
    pending.flushSync();
    assert.equal(pending.target.querySelector("button.primary").disabled, true, "no decision while the rows load");
  } finally {
    pending.destroy();
  }

  messages = () => [500, { error: "The store is busy." }];
  const view = await mount("/src/views/Request.svelte", { changeset: one });
  try {
    const alert = await until(() => view.target.querySelector("[role=alert]"));
    assert.match(alert.textContent, /The store is busy\./);
    assert.equal(view.target.querySelector("button.primary").disabled, true, "no decision without the rows");
    messages = () => [200, fx.answer("/messages?ids=m-1")];
    alert.querySelector("button").click();
    await until(() => view.target.querySelectorAll(".messages li").length === 1);
    view.flushSync();
    assert.equal(view.target.querySelector("[role=alert]"), null);
    assert.equal(view.target.querySelector("button.primary").disabled, false);
  } finally {
    view.destroy();
  }
});

test("a request of 501 changes goes in two batches; a refusal of the second says what Iris saved", async () => {
  const many = changeset(Array.from({ length: 501 }, (_, i) => act({ n: i + 1, hash: `h${i + 1}` })));
  const decided = (list) => ({ ...many, actions: many.actions.map((a) => (list.some((d) => d.n === a.n) ? { ...a, decision: { state: "approved", by: "person" } } : a)) });
  let calls = 0;
  const asked = serve({
    "changesets/cs-9/decide": (body) => (++calls === 1 ? [200, decided(body.actions)] : [409, { error: "the hash changed" }]),
    "changesets/cs-9": [200, decided(many.actions.slice(0, 500))],
  });
  const view = await mount("/src/views/Review.svelte", { changesets: [many] });
  try {
    await until(() => !view.target.querySelector("button.primary").disabled);
    assert.equal(view.target.querySelector("button.primary").textContent, "Approve all 501");
    view.target.querySelector("button.primary").click();
    const alert = await until(() => view.target.querySelector("[role=alert]"));
    await until(() => text(view.target).includes("asks for one change"));
    const sent = asked.filter((x) => Array.isArray(x)).map(([, b]) => b.actions.length);
    assert.deepEqual(sent, [500, 1]);
    assert.match(alert.textContent, /Iris saved your decision on the first 500 changes\. Iris did not save the rest, because the request changed/);
    assert.equal(view.target.querySelector("button.primary").textContent, "Approve the change");
  } finally {
    view.destroy();
  }
});

test("after a 409 with a failed refresh, the old request cannot be decided until it loads again", async () => {
  const before = three();
  let fresh = () => [500, { error: "The store is busy." }];
  const asked = serve({ "changesets/cs-9/decide": () => [409, { error: "the hash changed" }], "changesets/cs-9": () => fresh() });
  const view = await mount("/src/views/Review.svelte", { changesets: [before] });
  try {
    await until(() => !view.target.querySelector("button.primary").disabled);
    view.target.querySelector("button.primary").click();
    const alert = await until(() => view.target.querySelector("[role=alert] button"));
    assert.match(text(view.target.querySelector("[role=alert]")), /Iris did not save this decision.*could not load the request again/);
    assert.equal(view.target.querySelector("button.primary").disabled, true);
    fresh = () => [200, { ...before, actions: before.actions.map((a) => ({ ...a, decision: { state: "approved", by: "rule", rule: 2 } })) }];
    alert.click();
    await until(() => text(view.target).includes("Nothing waits for your review."));
    assert.equal(asked.filter((x) => Array.isArray(x)).length, 1, "one decision only");
  } finally {
    view.destroy();
  }
});

test("a decision with no answer loads the request again and does not say that nothing was saved", async () => {
  const before = three();
  const after = { ...before, state: "closed", actions: before.actions.map((a) => ({ ...a, decision: { state: "approved", by: "person" } })) };
  serve({
    "changesets/cs-9/decide": () => {
      throw new TypeError("The connection closed.");
    },
    "changesets/cs-9": [200, after],
  });
  const view = await mount("/src/views/Review.svelte", { changesets: [before] });
  try {
    await until(() => !view.target.querySelector("button.primary").disabled);
    view.target.querySelector("button.primary").click();
    const alert = await until(() => view.target.querySelector("[role=alert]"));
    await until(() => text(view.target).includes("Nothing waits for your review."));
    assert.match(alert.textContent, /does not know if it saved your decision/);
    assert.doesNotMatch(text(view.target), /did not save/);
    assert.equal(view.target.querySelector("[role=status]"), null);
    assert.equal(view.target.querySelector("button.primary"), null);
  } finally {
    view.destroy();
  }
});

test("the button waits for every row of a change of more than one page", async () => {
  serve();
  const answer = globalThis.fetch;
  let release;
  const later = new Promise((r) => (release = r));
  let pages = 0;
  globalThis.fetch = async (url, init) => {
    if (String(url).includes("messages?") && ++pages === 2) await later;
    return answer(url, init);
  };
  const ids = Array.from({ length: 250 }, (_, i) => `m-${i + 1}`);
  const view = await mount("/src/views/Request.svelte", { changeset: changeset([act({ resolved: { messages: ids } })]) });
  try {
    await until(() => pages === 2);
    view.flushSync();
    assert.equal(view.target.querySelector("button.primary").disabled, true, "no decision with 50 rows not loaded");
    assert.match(text(view.target), /Iris loads everything that these changes touch/);
    release();
    await until(() => !view.target.querySelector("button.primary").disabled);
    assert.equal(view.target.querySelectorAll(".messages li").length, 250, "each row shows, in a box that scrolls");
  } finally {
    view.destroy();
  }
});
