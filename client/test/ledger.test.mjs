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

test("who decided: the author, and the rule or the person that approved", () => {
  const person = { kind: "person", name: null, job: null, goal: null };
  const agent = { kind: "agent", name: "Claude Code", job: "j-4", goal: "Pay the invoice" };
  assert.equal(l.who(e({ author: person, decided_by: "person" }), {}), "You");
  assert.equal(l.who(e({ author: agent, decided_by: "person" }), {}), "An agent, job “Pay the invoice”, approved by you");
  assert.equal(l.who(e({ author: { ...agent, goal: null }, decided_by: "rule", rule: 3 }), { 3: "Tidy" }), "An agent, allowed by your rule “Tidy”");
  assert.equal(l.who(e({ author: { kind: "triage" }, decided_by: "rule", rule: 3 }), {}), "Iris triage, allowed by your rule 3");
  assert.equal(l.who(e({ author: { kind: "local", name: "cron" }, decided_by: "person" }), {}), "cron, approved by you");
});

test("a failed or a running entry says so; a done entry needs no words", () => {
  assert.equal(l.status(e({ state: "applied" })), "");
  assert.equal(l.status(e({ state: "writing" })), "Iris makes this change now.");
  assert.equal(l.status(e({ state: "failed", messages: 40, changed: 12, error: "Gmail refused one call." })), "Failed. 12 of 40 messages changed. Gmail refused one call.");
  assert.equal(l.status(e({ state: "failed", messages: 0, items: 1, error: null })), "Failed.");
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
