// The review screen (R2-06): the words of a changeset, and server renders with the fixtures.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, render } from "./render.mjs";

const fx = await load("/dev/fixtures.ts");
const r = await load("/src/review.ts");
const { parse } = await load("/src/route.ts");
const text = (html) => html.replace(/<!--[^>]*-->/g, "").replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");
const act = (a) => ({ n: 1, revision: 1, verb: "archive", targets: {}, resolved: { messages: [] }, args: {}, why: null, flags: [], hash: null, decision: { state: "in_review" }, ...a });

test("each verb reads as the start of a sentence, with its argument", () => {
  assert.equal(r.verb(act({ verb: "label", args: { label: "Finance" } })), "Add the label “Finance” to");
  assert.equal(r.verb(act({ verb: "unlabel", args: { label: "Old" } })), "Remove the label “Old” from");
  assert.equal(r.verb(act({ verb: "trash" })), "Move to the trash");
  // An argument that is not text shows nothing, never markup or an object.
  assert.equal(r.verb(act({ verb: "label", args: { label: { x: 1 } } })), "Add the label “” to");
});

test("the target counts the real messages, and names the threads only when there are two or more", () => {
  const ids = (n) => Array.from({ length: n }, (_, i) => `m-${i}`);
  assert.equal(r.target(act({ resolved: { messages: ids(1) } })), "1 message");
  assert.equal(r.target(act({ targets: { threads: ["t-1"] }, resolved: { messages: ids(2) } })), "2 messages");
  assert.equal(r.target(act({ targets: { threads: ["t-1", "t-2"] }, resolved: { messages: ids(1200) } })), "1,200 messages in 2 threads");
  assert.equal(r.target(act({ verb: "close", targets: { items: ["c-1"] } })), "1 item");
});

test("the review shows the newest revision and the open actions of an older one, never a replaced or draft action", () => {
  const c = {
    revision: 2,
    actions: [
      act({ n: 1, decision: { state: "replaced", by: "author" } }),
      act({ n: 2, decision: { state: "denied", by: "engine", reason: "x" } }),
      act({ n: 3, decision: { state: "approved", by: "rule", rule: 1 } }),
      act({ n: 1, revision: 2 }),
      act({ n: 2, revision: 2, decision: { state: "denied", by: "limit", reason: "Too many." } }),
      act({ n: 3, revision: 2, decision: null }),
    ],
  };
  assert.deepEqual(r.shown(c).map((a) => `${a.revision}.${a.n}`), ["1.3", "2.1", "2.2"]);
  assert.deepEqual(r.shown(c).filter(r.waits).map((a) => `${a.revision}.${a.n}`), ["2.1"]);
  assert.equal(r.outcome(c.actions[2]), "Approved by rule 1");
  assert.equal(r.outcome(c.actions[4]), "Denied: Too many.");
});

test("the review route opens the review", () => {
  assert.deepEqual(parse("#/review"), { view: "review" });
  assert.deepEqual(parse("#/handled"), { view: "handled" });
});

test("the review says who asks and why, and each change with its count and flags", async () => {
  const t = text(await render("/src/views/Review.svelte", { changesets: fx.changesets, mode: "observe" }));
  assert.match(t, /3 requests ask for 6 changes\./);
  assert.match(t, /Iris is in observe mode/);
  assert.match(t, /Claude Code .*Priya needs the Q4 numbers by Friday/);
  assert.match(t, /Add the label “Finance” to 2 messages The Q4 thread/);
  assert.match(t, /Archive 3 messages in 2 threads/);
  assert.match(t, /The thread is with a sender you do not know/);
  assert.match(t, /Iris triage .*Archive 612 messages Part of a change to more than 500 messages/);
  // The action that a rule approved is not a question: it shows under the decisions.
  assert.match(t, /One more change has a decision Archive 1 message Approved by rule 1/);
  assert.match(t, /Iris makes none of these changes before you approve them\./);
});

test("one request names its author, and a review with nothing to decide says so, but still shows each open request", async () => {
  const one = text(await render("/src/views/Review.svelte", { changesets: [fx.changesets[0]], mode: "apply" }));
  assert.match(one, /Claude Code asks for 3 changes\./);
  assert.doesNotMatch(one, /observe mode/);
  const closed = { ...fx.changesets[1], actions: [act({ decision: { state: "denied", by: "person", reason: "No." } })] };
  const none = text(await render("/src/views/Review.svelte", { changesets: [closed], mode: "apply" }));
  assert.match(none, /Nothing waits for your review\./);
  assert.match(none, /Iris triage/);
  assert.doesNotMatch(none, /When an agent or Iris asks/);
  const empty = text(await render("/src/views/Review.svelte", { changesets: [], mode: "apply" }));
  assert.match(empty, /Nothing waits for your review\. When an agent or Iris asks/);
});

test("the brief says how many changes wait for review and links the review", async () => {
  const html = await render("/src/views/Brief.svelte", { brief: fx.brief, now: new Date("2026-11-16T09:05:00Z") });
  assert.match(text(html), /Four changes wait for your review\./);
  assert.match(html, /href="#\/review"/);
  const none = await render("/src/views/Brief.svelte", { brief: { ...fx.brief, review: { changesets: 0, actions: 0 } }, now: new Date("2026-11-16T09:05:00Z") });
  assert.doesNotMatch(none, /#\/review|for your review/);
});

test("a state verb shows its item and the arguments that set its effect", async () => {
  assert.equal(r.detail(act({ verb: "close", args: { reason: "Paid on Friday." } })), "Reason: Paid on Friday.");
  assert.equal(r.detail(act({ verb: "correct", args: { field: "due", value: { date: "2026-11-20" } } })), 'New value: {"date":"2026-11-20"}');
  assert.equal(r.detail(act({ verb: "archive" })), "");
  const close = act({ verb: "close", targets: { items: ["c-7"] }, args: { reason: "Paid on Friday." } });
  assert.deepEqual(r.targets(close), ["c-7"]);
  assert.deepEqual(r.itemRow("c-7", { id: "c-7", form: "commitment", title: "Pay the invoice" }), { id: "c-7", gone: false, href: "#/item/c-7", who: "Commitment", what: "Pay the invoice", when: null });
  assert.deepEqual(r.itemRow("c-8", null), { id: "c-8", gone: true });
  const c = { ...fx.changesets[0], actions: [close] };
  const t = text(await render("/src/views/Review.svelte", { changesets: [c], mode: "apply" }));
  assert.match(t, /Close 1 item Reason: Paid on Friday\./);
});

test("the claim about the mailbox covers only the changes that wait for you", async () => {
  const t = text(await render("/src/views/Review.svelte", { changesets: fx.changesets, mode: "apply" }));
  assert.match(t, /Iris makes none of these changes before you approve them\./);
  assert.doesNotMatch(t, /Nothing changes in your mailbox/);
});

test("the pages of each action are keyed by revision and number, and a slow load adds nothing twice", async () => {
  const calls = [];
  const gates = [];
  const loader = r.pages((ids) => {
    calls.push(ids);
    return new Promise((done) => gates.push(() => done(ids.map((id) => ({ id, gone: true })))));
  });
  // Two revisions reuse the number 1.
  const old = act({ n: 1, revision: 1, resolved: { messages: ["m-1", "m-2"] } });
  const now = act({ n: 1, revision: 2, resolved: { messages: ["m-8", "m-9"] } });
  const a = loader.more(old, 1);
  const b = loader.more(now, 1);
  // A second load of a key while its first load runs starts nothing.
  assert.equal(await loader.more(old, 1), undefined);
  assert.equal(calls.length, 2);
  // The loads end in the other order.
  gates[1]();
  gates[0]();
  assert.deepEqual((await b).map((x) => x.id), ["m-8"]);
  assert.deepEqual((await a).map((x) => x.id), ["m-1"]);
  // The next page continues after the rows that each key has.
  const c = loader.more(now, 5);
  gates[2]();
  assert.deepEqual((await c).map((x) => x.id), ["m-8", "m-9"]);
  assert.deepEqual(calls, [["m-1"], ["m-8"], ["m-9"]]);
  // With all rows loaded, nothing more is asked.
  assert.deepEqual((await loader.more(now, 5)).map((x) => x.id), ["m-8", "m-9"]);
  assert.equal(calls.length, 3);
});

test("the decide button says what happens to the ticked and the unticked changes", () => {
  assert.equal(r.plan(3, 3), "Approve all 3");
  assert.equal(r.plan(1, 1), "Approve the change");
  assert.equal(r.plan(2, 3), "Approve 2, deny 1");
  assert.equal(r.plan(0, 3), "Deny all 3");
  assert.equal(r.plan(0, 1), "Deny the change");
});

test("an item with the status gone shows as gone, and the bulk flag names no count of the action", () => {
  const item = { ...fx.items[0], status: "gone" };
  assert.deepEqual(r.itemRow(item.id, item), { id: item.id, gone: true });
  assert.equal(r.itemRow(fx.items[0].id, fx.items[0]).gone, false);
  // Spec 2 section 5.3: the flag can come from the changeset or the author, so a small action can have it.
  assert.doesNotMatch(r.FLAGS.bulk, /^Changes/);
  assert.match(r.FLAGS.bulk, /Part of a change to more than 500 messages/);
});

// Card R4-07: a draft or a send in words, and the flags that its draft does not mark.
test("a draft or a send names its kind and its recipients", () => {
  assert.equal(r.verb(act({ verb: "send" })), "Send");
  assert.equal(r.verb(act({ verb: "draft" })), "Write the draft of");
  const d = (to, cc = [], thread = null) => ({ revision: 1, n: 1, draft: null, thread, readable: true, to: to.map((address) => ({ address })), cc: cc.map((address) => ({ address })), subject: [], body: [] });
  assert.equal(r.to(d(["a@x.org"], [], "t-1")), "a reply to a@x.org");
  assert.equal(r.to(d(["a@x.org"], ["b@x.org"])), "a new mail to a@x.org and b@x.org");
  assert.equal(r.to(d(["a@x.org", "b@x.org"], ["c@x.org"])), "a new mail to a@x.org and 2 more");
  assert.equal(r.to({ revision: 1, n: 1, draft: "d-1", thread: null, readable: false }), "a new mail");
});

test("a flag that the draft marks has no line; a flag with no mark keeps its words", () => {
  const a = act({ verb: "send", flags: ["new_recipient", "money", "first_contact"] });
  const d = { revision: 1, n: 1, draft: "d-1", thread: null, readable: true, to: [{ address: "a@x.org" }], cc: [], subject: [{ text: "Hi" }], body: [{ text: "$5", flags: ["money"] }] };
  assert.deepEqual(r.unmarked(a, d), [r.FLAGS.new_recipient, r.FLAGS.first_contact]);
  // No draft yet, or a draft that Iris cannot read: each flag keeps its words.
  assert.deepEqual(r.unmarked(a, undefined).length, 3);
  assert.deepEqual(r.unmarked(a, { revision: 1, n: 1, draft: "d-1", thread: null, readable: false }).length, 3);
  // Each flag has a tag.
  for (const f of ["new_recipient", "money", "date", "attachment", "untrusted_content"]) assert.ok(r.TAGS[f], f);
});
