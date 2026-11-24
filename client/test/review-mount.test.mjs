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
    // The comment box of an agent author loads its comments (decision D15).
    const route = routes[path] ?? (path.endsWith("/comments") ? [200, { comments: [] }] : undefined);
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
    // No approve button: the decision row holds only the withdraw.
    assert.equal(view.target.querySelector("button.primary"), null);
    assert.ok(view.target.querySelector(".decide button.end"));
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

test("withdraw asks once, then cancels each change that Iris did not make", async () => {
  // One change waits, one is approved and waits for apply mode, one is done.
  const before = changeset([
    act({ n: 1, resolved: { messages: ["m-1"] } }),
    act({ n: 2, decision: { state: "approved", by: "person" } }),
    act({ n: 3, decision: { state: "applied", by: "person" } }),
  ]);
  const after = { ...before, state: "withdrawn", actions: before.actions.map((a) => (a.n < 3 ? { ...a, decision: { state: "cancelled", by: "author" } } : a)) };
  const asked = serve({ "changesets/cs-9/withdraw": [200, after] });
  const view = await mount("/src/views/Review.svelte", { changesets: [before] });
  try {
    const control = () => view.target.querySelector("button.end");
    // "Keep it" closes the question and sends nothing.
    control().click();
    view.flushSync();
    assert.match(text(view.target.querySelector(".question")), /Withdraw this request\? Iris cancels each change of this request that it did not start\. A change that Iris made or started stays in your mailbox, also the part of a failed change that Iris made before it failed\. You cannot undo a withdraw\./);
    view.target.querySelector(".question button.quiet").click();
    view.flushSync();
    assert.equal(view.target.querySelector(".question"), null);
    assert.equal(asked.filter((x) => Array.isArray(x)).length, 0);

    control().click();
    view.flushSync();
    view.target.querySelector(".question button.outline").click();
    await until(() => text(view.target).includes("You withdrew this request. Iris cancelled 2 changes."));
    assert.doesNotMatch(text(view.target), /started|This request is withdrawn/);
    assert.deepEqual(asked.filter((x) => Array.isArray(x)).map(([p]) => p), ["changesets/cs-9/withdraw"]);
    assert.equal(control(), null, "a withdrawn request has no control");
    assert.match(text(view.target), /Nothing waits for your review\./);
    assert.equal(view.target.querySelectorAll(".decided li").length, 3);
    assert.match(text(view.target.querySelector(".decided")), /Withdrawn.*Withdrawn.*Done/);
  } finally {
    view.destroy();
  }
});

test("a withdraw counts from the states that the store gives back: a started change goes on", async () => {
  // Iris started change 2 (applying), so the store keeps it approved. Change 3 failed.
  const before = changeset([
    act({ n: 1 }),
    act({ n: 2, decision: { state: "approved", by: "person" } }),
    act({ n: 3, decision: { state: "failed", by: "person" } }),
  ]);
  const after = { ...before, state: "withdrawn", actions: before.actions.map((a) => (a.n === 1 ? { ...a, decision: { state: "cancelled", by: "author" } } : a)) };
  serve({ "changesets/cs-9/withdraw": [200, after] });
  const view = await mount("/src/views/Review.svelte", { changesets: [before] });
  try {
    view.target.querySelector("button.end").click();
    view.flushSync();
    // The question promises no count: the page cannot see which approved change Iris started.
    assert.doesNotMatch(text(view.target.querySelector(".question")), /\d/);
    assert.match(text(view.target.querySelector(".question")), /the part of a failed change that Iris made before it failed/);
    view.target.querySelector(".question button.outline").click();
    await until(() => text(view.target).includes("You withdrew this request."));
    assert.match(text(view.target), /Iris cancelled one change\. Iris started one change before the withdraw, and it goes on to the end\./);
  } finally {
    view.destroy();
  }
});

test("a withdraw keeps each change that it cancelled in view, also one of an older revision", async () => {
  // Revision 2 came while change 1 of revision 1 was approved: it stays open (spec 2 section 5.5).
  const before = { ...changeset([act({ n: 1, revision: 1, decision: { state: "approved", by: "person" } }), act({ n: 1, revision: 2, verb: "read" })]), revision: 2 };
  const after = { ...before, state: "withdrawn", actions: before.actions.map((a) => ({ ...a, decision: { state: "cancelled", by: "author" } })) };
  serve({ "changesets/cs-9/withdraw": [200, after] });
  const view = await mount("/src/views/Review.svelte", { changesets: [before] });
  try {
    view.target.querySelector("button.end").click();
    view.flushSync();
    view.target.querySelector(".question button.outline").click();
    await until(() => text(view.target).includes("Iris cancelled 2 changes."));
    const rows = [...view.target.querySelectorAll(".decided li")].map(text);
    assert.equal(rows.length, 2, "the change of revision 1 stays in view");
    for (const row of rows) assert.match(row, /Withdrawn/);
  } finally {
    view.destroy();
  }
});

test("withdraw sits at the end of the decision row, also when no change waits", async () => {
  serve();
  const waiting = await mount("/src/views/Request.svelte", { changeset: changeset([act({ n: 1 })]) });
  try {
    assert.ok(waiting.target.querySelector(".decide button.end"), "after the approve button");
    assert.equal(waiting.target.querySelector("header button"), null, "not in the head");
  } finally {
    waiting.destroy();
  }
  const approved = await mount("/src/views/Request.svelte", { changeset: changeset([act({ n: 1, decision: { state: "approved", by: "person" } })]) });
  try {
    assert.match(text(approved.target.querySelector(".decide")), /No change of this request waits for your decision\. Withdraw the request/);
  } finally {
    approved.destroy();
  }
});

test("a withdraw that is refused or has no answer loads the request again and says so", async () => {
  const before = three();
  const gone = { ...before, state: "withdrawn", actions: before.actions.map((a) => ({ ...a, decision: { state: "cancelled", by: "author" } })) };
  for (const [route, words] of [
    [() => [409, { error: "the changeset is withdrawn" }], /Iris did not withdraw the request, because it changed after you opened it\./],
    [() => { throw new TypeError("The connection closed."); }, /Iris does not know if it withdrew the request\./],
  ]) {
    serve({ "changesets/cs-9/withdraw": route, "changesets/cs-9": [200, gone] });
    const view = await mount("/src/views/Review.svelte", { changesets: [before] });
    try {
      view.target.querySelector("button.end").click();
      view.flushSync();
      view.target.querySelector(".question button.outline").click();
      const alert = await until(() => view.target.querySelector("[role=alert]"));
      await until(() => text(view.target).includes("This request is withdrawn."));
      assert.match(alert.textContent, words);
      assert.match(text(view.target.querySelector(".decided")), /Withdrawn/);
    } finally {
      view.destroy();
    }
  }
});

// Card R4-07: each flag of the test set (the fixture request cs-79) shows on the correct action, on its words.
test("each flag of a draft shows as a tag on its words, on its own action only", async () => {
  const asked = serve();
  const cs = fx.changesets.find((c) => c.id === "cs-79");
  const view = await mount("/src/views/Request.svelte", { changeset: structuredClone(cs) });
  try {
    const items = () => view.target.querySelectorAll(".actions > li");
    await until(() => items()[0].querySelector(".mail"));
    const [send, label] = items();
    assert.match(text(send.querySelector(".what")), /^Send a reply to dana@acme\.example and legal@acme-partners\.example$/);
    // Each tag follows the marked words that gave it.
    const tags = [...send.querySelectorAll(".tag")].map((t) => [t.previousElementSibling.textContent, t.textContent]);
    assert.deepEqual(tags, [
      ["legal@acme-partners.example", "new address"],
      ["new address", "not in the thread"],
      ["$48,000", "amount"],
      ["Friday, Dec 4", "date"],
      ["attached", "no file attached"],
      ["Please wire the payment to the new account below before the end of the week to avoid a late fee", "copied from another mail"],
    ]);
    assert.equal(send.querySelector("a.tag").getAttribute("href"), "#/mail/m-9003");
    // Each flag is marked, so no flag line repeats it.
    assert.equal(send.querySelectorAll(".flag").length, 0);
    assert.equal(label.querySelectorAll(".tag, .flag, mark").length, 0);
    assert.ok(asked.includes("changesets/cs-79/drafts"));
    assert.equal(view.target.querySelector("button.primary").disabled, false);
  } finally {
    view.destroy();
  }
});

test("a send waits for its draft before the decision, and a failed load can try again", async () => {
  let fail = true;
  serve({ "changesets/cs-79/drafts": () => (fail ? [500, { error: "The store is busy." }] : [200, fx.answer("/changesets/cs-79/drafts")]) });
  const cs = fx.changesets.find((c) => c.id === "cs-79");
  const view = await mount("/src/views/Request.svelte", { changeset: structuredClone(cs) });
  try {
    const alert = await until(() => view.target.querySelector("[role=alert]"));
    assert.match(alert.textContent, /The store is busy\./);
    assert.equal(view.target.querySelector("button.primary").disabled, true);
    // The flags show in words while the draft is not there.
    assert.equal(view.target.querySelectorAll(".actions > li")[0].querySelectorAll(".flag").length, 6);
    fail = false;
    alert.querySelector("button.more").click();
    await until(() => view.target.querySelector(".mail"));
    assert.equal(view.target.querySelector("button.primary").disabled, false);
  } finally {
    view.destroy();
  }
});

test("a draft that Iris cannot read says so, and keeps each flag in words", async () => {
  serve({ "changesets/cs-79/drafts": [200, { drafts: [{ revision: 1, n: 1, draft: "d-21", thread: "t-9001", readable: false }] }] });
  const cs = fx.changesets.find((c) => c.id === "cs-79");
  const view = await mount("/src/views/Request.svelte", { changeset: structuredClone(cs) });
  try {
    await until(() => text(view.target).includes("Iris cannot show the text of this draft"));
    const send = view.target.querySelectorAll(".actions > li")[0];
    assert.equal(send.querySelectorAll(".flag").length, 6);
    assert.equal(send.querySelector(".mail"), null);
    // Spec 2 section 5.5: no approval without the final text. The person can only deny it.
    const box = send.querySelector("input[type=checkbox]");
    assert.equal(box.checked, false);
    assert.equal(box.disabled, true);
    assert.match(text(send), /Iris will deny this change\./);
    const button = await until(() => {
      const b = view.target.querySelector("button.primary");
      return !b.disabled && b;
    });
    assert.equal(button.textContent, "Approve 1, deny 1");
  } finally {
    view.destroy();
  }
});

test("a reply shows the message that it answers, and waits for it before the decision", async () => {
  let open;
  const gate = new Promise((done) => (open = done));
  serve({ "messages?ids=m-9002": async () => (await gate, [200, fx.answer("/messages?ids=m-9002")]) });
  const cs = fx.changesets.find((c) => c.id === "cs-79");
  const view = await mount("/src/views/Request.svelte", { changeset: structuredClone(cs) });
  try {
    const send = () => view.target.querySelectorAll(".actions > li")[0];
    await until(() => send().querySelector(".mail"));
    assert.equal(view.target.querySelector("button.primary").disabled, true);
    open();
    await until(() => send().querySelector(".messages a[href]"));
    await until(() => !view.target.querySelector("button.primary").disabled);
  } finally {
    view.destroy();
  }
});

test("a drafts answer with no entry for a waiting send loads the request again and offers a retry", async () => {
  const asked = serve({ "changesets/cs-79/drafts": [200, { drafts: [] }] });
  const cs = fx.changesets.find((c) => c.id === "cs-79");
  const view = await mount("/src/views/Request.svelte", { changeset: structuredClone(cs) });
  try {
    const alert = await until(() => view.target.querySelector("[role=alert]"));
    assert.match(alert.textContent, /This request changed after you opened it\./);
    assert.ok(asked.includes("changesets/cs-79"));
    assert.equal(view.target.querySelector("button.primary").disabled, true);
    assert.ok(alert.querySelector("button.more"), "a retry");
  } finally {
    view.destroy();
  }
});

test("a decided send names the mail, not a count of messages", async () => {
  serve();
  const cs = structuredClone(fx.changesets.find((c) => c.id === "cs-79"));
  cs.actions[0].decision = { state: "denied", by: "person", reason: "No." };
  const view = await mount("/src/views/Request.svelte", { changeset: cs });
  try {
    const row = await until(() => [...view.target.querySelectorAll(".decided li")].find((li) => li.textContent.includes("Send")));
    assert.equal(text(row), "Send a mail You denied it");
  } finally {
    view.destroy();
  }
});

// Issue 189: an open change of an older revision shows the reason of its own revision.
test("an open change of an older revision shows the reason of its revision", async () => {
  const c = { ...changeset([act({ n: 1, revision: 1 }), act({ n: 1, revision: 2, verb: "read", hash: "h2" })]), revision: 2, why: "Also mark the news as read." };
  const asked = serve({ "changesets/cs-9/revisions": [200, { revisions: [{ revision: 1, why: "Tidy the inbox.", submitted: "2026-11-15T08:00:00Z" }, { revision: 2, why: "Also mark the news as read.", submitted: "2026-11-15T09:00:00Z" }] }] });
  const view = await mount("/src/views/Request.svelte", { changeset: c });
  try {
    const rows = () => [...view.target.querySelectorAll(".actions > li")].map(text);
    await until(() => /which asked/.test(rows().join()));
    assert.match(rows()[0], /From revision 1, which asked: “Tidy the inbox\.”/);
    assert.doesNotMatch(rows()[1], /From revision/, "a change of the newest revision has the quote above");
    assert.ok(asked.includes("changesets/cs-9/revisions"));
  } finally {
    view.destroy();
  }
  // With no answer, the older change still names its revision.
  serve({ "changesets/cs-9/revisions": () => { throw new TypeError("Failed to fetch"); } });
  const lost = await mount("/src/views/Request.svelte", { changeset: c });
  try {
    await until(() => /From revision 1\./.test(text(lost.target)));
  } finally {
    lost.destroy();
  }
});

test("a request with no older change does not ask for the revisions", async () => {
  const asked = serve();
  const view = await mount("/src/views/Request.svelte", { changeset: changeset([act({})]) });
  try {
    await until(() => view.target.querySelector(".actions"));
    assert.ok(!asked.includes("changesets/cs-9/revisions"));
    assert.doesNotMatch(text(view.target), /From revision/);
  } finally {
    view.destroy();
  }
});

// Decision D16: a withdrawn request names who withdrew it. A store from before the change does not know.
test("a withdrawn request names who withdrew it", async () => {
  const before = three();
  for (const [by, words] of [
    ["author", "Claude Code withdrew this request."],
    ["person", "You withdrew this request."],
    [undefined, "This request is withdrawn."],
  ]) {
    const gone = { ...before, state: "withdrawn", withdrawn_by: by, actions: before.actions.map((a) => ({ ...a, decision: { state: "cancelled", by: "author" } })) };
    serve({ "changesets/cs-9/withdraw": () => [409, { error: "the changeset is withdrawn" }], "changesets/cs-9": [200, gone] });
    const view = await mount("/src/views/Review.svelte", { changesets: [before] });
    try {
      view.target.querySelector("button.end").click();
      view.flushSync();
      view.target.querySelector(".question button.outline").click();
      await until(() => text(view.target).includes(words));
    } finally {
      view.destroy();
    }
  }
});

test("in observe mode, a proposed change says what a rule would decide in apply mode", async () => {
  const asked = serve({
    "changesets/cs-9/would": [200, { would: [{ revision: 1, n: 2, outcome: "approved", rule: 3, policy: 1, reason: "Rule 3 allows it." }] }],
  });
  const proposed = { state: "proposed" };
  const view = await mount("/src/views/Request.svelte", {
    changeset: changeset([act({ decision: proposed }), act({ n: 2, decision: proposed })]),
  });
  try {
    const items = () => view.target.querySelectorAll(".actions > li");
    await until(() => view.target.querySelector(".would"));
    assert.equal(items()[0].querySelector(".would"), null, "no rule decides the first change");
    assert.equal(items()[1].querySelector(".would").textContent, "In apply mode, rule 3 would approve this change.");
    assert.equal(asked.filter((p) => p === "changesets/cs-9/would").length, 1);
  } finally {
    view.destroy();
  }
});

test("a request with no proposed change does not ask what a rule would decide", async () => {
  const asked = serve();
  const view = await mount("/src/views/Request.svelte", { changeset: changeset([act({ resolved: { messages: ["m-1"] } })]) });
  try {
    await until(() => view.target.querySelectorAll(".messages li").length === 1);
    assert.ok(!asked.includes("changesets/cs-9/would"));
  } finally {
    view.destroy();
  }
});

test("a new revision of the request reads again what a rule would decide, and drops the old lines", async () => {
  const proposed = { state: "proposed" };
  const before = changeset([act({ decision: proposed, resolved: { messages: ["m-1"] } })]);
  const after = { ...before, revision: 2, actions: [{ ...before.actions[0], decision: { state: "replaced", by: "author" } }, act({ revision: 2, hash: "h2", decision: proposed, resolved: { messages: ["m-1"] } })] };
  let answer = { would: [{ revision: 1, n: 1, outcome: "approved", rule: 3, policy: 1, reason: "r" }] };
  const asked = serve({
    "changesets/cs-9/would": () => [200, answer],
    "changesets/cs-9/decide": () => [409, { error: "the hash changed" }],
    "changesets/cs-9": [200, after],
  });
  const view = await mount("/src/views/Review.svelte", { changesets: [before] });
  try {
    await until(() => view.target.querySelector(".would"));
    assert.match(view.target.querySelector(".would").textContent, /rule 3 would approve/);
    answer = { would: [{ revision: 2, n: 1, outcome: "denied", rule: 4, policy: 2, reason: "r" }] };
    await until(() => !view.target.querySelector("button.primary").disabled);
    view.target.querySelector("button.primary").click();
    await until(() => view.target.querySelector(".would")?.textContent.includes("rule 4"));
    assert.equal(view.target.querySelector(".would").textContent, "In apply mode, rule 4 would deny this change.");
    assert.equal(asked.filter((p) => p === "changesets/cs-9/would").length, 2);
  } finally {
    view.destroy();
  }
});

test("a refresh of the same revision reads again what a rule would decide, and drops the old lines", async () => {
  // The policy can change between two loads of the same revision: the line must follow it.
  const proposed = { state: "proposed" };
  const before = changeset([act({ decision: proposed, resolved: { messages: ["m-1"] } })]);
  const same = { ...before, actions: before.actions.map((a) => ({ ...a })) };
  let answer = { would: [{ revision: 1, n: 1, outcome: "approved", rule: 3, policy: 1, reason: "r" }] };
  const asked = serve({
    "changesets/cs-9/would": () => [200, answer],
    "changesets/cs-9/decide": () => [409, { error: "the hash changed" }],
    "changesets/cs-9": [200, same],
  });
  const view = await mount("/src/views/Review.svelte", { changesets: [before] });
  try {
    await until(() => view.target.querySelector(".would"));
    assert.match(view.target.querySelector(".would").textContent, /rule 3 would approve/);
    answer = { would: [] };
    await until(() => !view.target.querySelector("button.primary").disabled);
    view.target.querySelector("button.primary").click();
    await until(() => asked.filter((p) => p === "changesets/cs-9/would").length === 2);
    await until(() => !view.target.querySelector(".would"));
    assert.equal(view.target.querySelector(".would"), null, "no rule decides it now");
  } finally {
    view.destroy();
  }
});

test("a draft shows its Bcc and its files (send content spec)", async () => {
  const d = structuredClone(fx.answer("/changesets/cs-79/drafts"));
  d.drafts[0].bcc = [{ address: "lee@example.org", flags: ["new_recipient"] }];
  d.drafts[0].attachments = [{ name: "quote.pdf", type: "application/pdf", size: 2_516_582 }];
  serve({ "changesets/cs-79/drafts": [200, d] });
  const cs = fx.changesets.find((c) => c.id === "cs-79");
  const view = await mount("/src/views/Request.svelte", { changeset: structuredClone(cs) });
  try {
    const mail = await until(() => view.target.querySelector(".mail"));
    const rows = [...mail.querySelectorAll("dt")].map((dt) => [dt.textContent, text(dt.nextElementSibling)]);
    assert.deepEqual(rows.slice(2), [
      ["Bcc", "lee@example.orgnew address"],
      ["Files", "quote.pdf (2.4 MB)"],
    ]);
  } finally {
    view.destroy();
  }
});
