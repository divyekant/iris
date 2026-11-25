// The words of the history view (R2-07): each entry as a sentence, and the day groups.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load } from "./render.mjs";

const fx = await load("/dev/fixtures.ts");
const l = await load("/src/ledger.ts");
const { parse, NAV } = await load("/src/route.ts");
const base = fx.ledger.entries[1];
const e = (x) => ({ ...base, ...x });

test("each verb reads in the past tense, with what it changed", () => {
  const words = (x) => `${l.did(e(x))} ${l.what(e(x))}`;
  assert.equal(words({ verb: "archive", messages: 1 }), "Archived 1 message");
  assert.equal(words({ verb: "label", args: { label: "Finance" }, messages: 3 }), "Added the label “Finance” to 3 messages");
  assert.equal(words({ verb: "unlabel", args: { label: "Finance" }, messages: 1200 }), "Removed the label “Finance” from 1,200 messages");
  assert.equal(words({ verb: "untrash", messages: 2 }), "Took out of the trash 2 messages");
  assert.equal(words({ verb: "close", items: 1, title: "Pay Acme" }), "Closed “Pay Acme”");
  assert.equal(words({ verb: "defer", args: { until: "2026-11-20" }, items: 2, title: "x" }), "Deferred to 2026-11-20 2 items");
  assert.equal(words({ verb: "send" }), "Sent a mail");
});

test("a send and a draft name their subject and first recipient, so two sends differ (issue 490)", () => {
  const words = (x) => `${l.did(e(x))} ${l.what(e(x))}`;
  assert.equal(words({ verb: "send", mail: { subject: "Q4 plan", to: "priya@example.com", recipients: 1 } }), "Sent “Q4 plan” to priya@example.com");
  assert.equal(words({ verb: "send", mail: { subject: "Q4 plan", to: "priya@example.com", recipients: 2 } }), "Sent “Q4 plan” to priya@example.com and 1 other");
  assert.equal(words({ verb: "draft", mail: { subject: " ", to: "dana@example.com", recipients: 4 } }), "Wrote the draft of a mail with no subject to dana@example.com and 3 others");
  // A change of a draft and its undo (draft change spec, section 6).
  const q4 = { subject: "Q4 plan", to: "priya@example.com", recipients: 1 };
  assert.equal(words({ verb: "draft", change: true, mail: q4 }), "Changed the draft of “Q4 plan” to priya@example.com");
  assert.equal(words({ verb: "draft", change: true, undo_of: "l-4", mail: q4 }), "Restored the old text of “Q4 plan” to priya@example.com");
  assert.equal(words({ verb: "draft", change: false, mail: q4 }), "Wrote the draft of “Q4 plan” to priya@example.com");
  // A mail that Iris cannot read, or an engine from before the field: no name.
  assert.equal(words({ verb: "send", mail: null }), "Sent a mail");
});

// Each verb that a changeset (and so the ledger) can hold has words: the list comes from the
// schema, so that a new verb without words makes this test fail.
test("each verb of the changeset schema has past-tense words", async () => {
  const { readFile } = await import("node:fs/promises");
  const schema = JSON.parse(await readFile(new URL("../../docs/spec/schema/changeset.schema.json", import.meta.url), "utf8"));
  const verbs = schema.$defs.verb.enum;
  assert.ok(verbs.includes("restore") && verbs.includes("mute"));
  for (const verb of verbs) {
    const said = l.did(e({ verb, args: { label: "L", until: "2026-11-20" } }));
    assert.match(said, /^[A-Z]/, verb);
    assert.notEqual(said.toLowerCase(), verb, verb);
  }
  const words = (x) => `${l.did(e(x))} ${l.what(e(x))}`;
  assert.equal(words({ verb: "mute", items: 1, title: "Lunch plans" }), "Muted “Lunch plans”");
  assert.equal(words({ verb: "restore", items: 1, title: "Lunch plans" }), "Restored “Lunch plans”");
});

test("who decided: the author, and the rule or the person that approved", () => {
  const person = { kind: "person", name: null, job: null, goal: null };
  const agent = { kind: "agent", name: "Claude Code", job: "j-4", goal: "Pay the invoice" };
  assert.equal(l.who(e({ author: person, decided_by: "person" })), "You");
  assert.equal(l.who(e({ author: agent, decided_by: "person" })), "An agent, job “Pay the invoice”, approved by you");
  // A rule decision shows the reason that the engine recorded, with the name of the rule at that time.
  assert.equal(l.who(e({ author: { ...agent, goal: null }, decided_by: "rule", rule: 3, reason: "Rule 3 (Tidy) allows archive." })), "An agent. Rule 3 (Tidy) allows archive.");
  assert.equal(l.who(e({ author: { kind: "triage" }, decided_by: "rule", rule: 3, reason: null })), "Iris triage, allowed by your rule 3");
  assert.equal(l.who(e({ author: { kind: "local", name: "cron" }, decided_by: "person" })), "cron, approved by you");
});

test("a failed or a running entry says so; a done entry needs no words", () => {
  assert.equal(l.status(e({ state: "applied" })), "");
  assert.equal(l.status(e({ state: "applied", error: "Sent. The draft stays in Outlook: delete it there." })), "Sent. The draft stays in Outlook: delete it there.");
  assert.equal(l.status(e({ state: "writing" })), "Iris makes this change now.");
  assert.equal(l.status(e({ state: "failed", messages: 40, changed: 12, error: "Gmail refused one call." })), "Failed. 12 of 40 messages changed. Gmail refused one call.");
  assert.equal(l.status(e({ state: "failed", messages: 0, items: 1, error: null })), "Failed.");
});

test("an undo that skipped messages says so, and the change keeps its Undo (D18)", () => {
  const u = (x) => e({ verb: "inbox", undo_of: "l-23", state: "applied", error: null, retry: true, ...x });
  // The count is each target of the change, also a message that was gone before the undo started.
  assert.equal(`${l.did(u({ messages: 0, tried: 4, skipped: 4 }))} ${l.what(u({ messages: 0, tried: 4, skipped: 4 }))}`, "Moved to the inbox 4 messages");
  assert.equal(l.what(u({ messages: 1, changed: 1, tried: 2, skipped: 1 })), "2 messages");
  assert.equal(l.status(u({ messages: 0, changed: 0, tried: 4, skipped: 4 })), "Nothing changed. The messages were already changed back, changed again after that change, or are gone. That change keeps its Undo.");
  // A message that the undo listed and did not change counts once.
  assert.equal(l.what(u({ messages: 1, changed: 0, tried: 1, skipped: 1 })), "1 message");
  assert.equal(l.status(u({ messages: 1, changed: 0, tried: 1, skipped: 1 })), "Nothing changed. The message was already changed back, changed again after that change, or is gone. That change keeps its Undo.");
  // No Undo is promised when the change has none now (each message is gone).
  assert.equal(l.status(u({ messages: 1, changed: 0, tried: 1, skipped: 1, retry: false })), "Nothing changed. The message was already changed back, changed again after that change, or is gone.");
  assert.equal(l.status(u({ messages: 1, changed: 1, tried: 3, skipped: 2 })), "2 messages stayed as they were: already changed back, changed again after that change, or gone.");
  assert.equal(l.status(u({ verb: "restore", items: 1, messages: 0, tried: 1, skipped: 1 })), "Nothing changed. The item was already changed back, changed again after that change, or is gone. That change keeps its Undo.");
  assert.equal(l.status(u({ messages: 3, changed: 3, tried: 3, skipped: 0 })), "");
  // A retry of a trash undo: step 1 skips the message that an earlier undo restored, and the change is undone.
  assert.equal(l.status(u({ verb: "untrash", messages: 1, changed: 0, tried: 1, skipped: 1, retry: false })), "Nothing changed. The message was already changed back, changed again after that change, or is gone.");
  // An entry that is no undo never counts its tried targets.
  assert.equal(l.what(e({ undo_of: null, messages: 2, tried: 5 })), "2 messages");
});

test("the entries group by the local day, in the order of the list", () => {
  const now = new Date(2026, 10, 17, 12, 0);
  const at = (d, h) => new Date(2026, 10, d, h, 0).toISOString();
  const list = [e({ id: "l-4", at: at(17, 9) }), e({ id: "l-3", at: at(17, 1) }), e({ id: "l-2", at: at(16, 23) }), e({ id: "l-1", at: at(10, 9) })];
  const groups = l.byDay(list, now);
  assert.deepEqual(groups.map((g) => [g.day, g.entries.map((x) => x.id)]).slice(0, 2), [["Today", ["l-4", "l-3"]], ["Yesterday", ["l-2"]]]);
  assert.equal(groups.length, 3);
  assert.match(groups[2].day, /10/);
  assert.equal(l.day(new Date(2025, 0, 2).toISOString(), now).includes("2025"), true);
});

test("the History link opens the view", () => {
  assert.deepEqual(parse("#/history"), { view: "history" });
  assert.ok(NAV.some(([link, label]) => link === "#/history" && label === "History"));
});
