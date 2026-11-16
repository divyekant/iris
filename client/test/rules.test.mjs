// The policy view (R2-08): words for rules, each state of the file, and the POST of a change.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, render } from "./render.mjs";

const fx = await load("/dev/fixtures.ts");
const text = (html) => html.replace(/<!--[^>]*-->/g, "").replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");
const same = { ...fx.policy, file: { state: "same" } };

test("a rule reads as words", async () => {
  const { verbs, conditions, list } = await load("/src/rules.ts");
  const [archive, hide] = fx.policy.rules;
  assert.equal(verbs(archive), "Archive and mark as read");
  assert.deepEqual(conditions(archive), ["Feed", "From services", "Older than 3 days"]);
  assert.equal(verbs(hide), "");
  assert.deepEqual(conditions(hide), ["From deals@acme.example"]);
  assert.deepEqual(conditions({ ...hide, when: { record_type: "receipt", commitment_kind: "reply", older_than_days: 1, author: "agent" }, job: "j-1" }), [
    "Receipts",
    "Replies",
    "Older than 1 day",
    "By agent",
    "Job j-1",
  ]);
  assert.equal(list(["a", "b", "c"]), "a, b and c");
});

test("the rules show in groups, strict first, each with its words and a remove button", async () => {
  const html = await render("/src/views/Rules.svelte", { policy: same });
  const t = text(html);
  const order = ["Ask first", "Allowed", "Hidden", "Bundled"].map((g) => t.indexOf(g));
  assert.deepEqual([...order].sort((a, b) => a - b), order);
  assert.ok(!order.includes(-1), t);
  assert.doesNotMatch(html, /id="deny"/);
  assert.match(t, /Archive newsletters after 3 days Archive and mark as read Feed From services Older than 3 days Rule 1 · line 10/);
  assert.match(t, /Rule 2 · line 19 · Iris proposed it/);
  assert.doesNotMatch(t, /policy file changed/);
  assert.doesNotMatch(html, /disabled/);
  assert.match(html, /aria-label="Remove Hide Acme Deals"/);
  assert.match(html, /value="observe" checked/);
});

test("a changed file shows its difference, and blocks the changes from here until it is confirmed or discarded", async () => {
  const html = await render("/src/views/Rules.svelte", { policy: fx.policy });
  const t = text(html);
  assert.match(t, /Your policy file changed/);
  assert.match(t, /Mode Observe to Apply/);
  assert.match(t, /New Mark receipts as read Mark as read Records Receipts/);
  assert.match(t, /Changed Archive newsletters after 5 days .* Before: Archive newsletters after 3 days, Archive and mark as read, Feed, From services, Older than 3 days/);
  assert.match(t, /Confirm the change Discard/);
  assert.match(t, /Confirm or discard the change of your file first/);
  // Each mode button and each remove button waits.
  assert.equal(html.match(/name="mode"[^>]*disabled|disabled[^>]*name="mode"/g)?.length, 2);
  assert.equal(html.match(/class="quiet remove[^"]*"[^>]*disabled/g)?.length, 4);
  const comments = { ...fx.policy, file: { ...fx.policy.file, mode: "observe", added: [], changed: [] } };
  assert.match(text(await render("/src/views/Rules.svelte", { policy: comments })), /Only comments or spaces changed/);
});

test("a file that is not valid shows its line, and an active text that is not valid says that Iris applies nothing", async () => {
  const bad = { ...same, file: { state: "invalid", error: { line: 22, message: 'the effect "hid" is not known' } } };
  const t = text(await render("/src/views/Rules.svelte", { policy: bad }));
  assert.match(t, /Your policy file has an error Line 22: the effect "hid" is not known/);
  assert.match(t, /Discard the file/);
  const broken = { ...same, error: { line: 3, message: "mode is not known" } };
  assert.match(text(await render("/src/views/Rules.svelte", { policy: broken })), /error on line 3: mode is not known .* Iris applies nothing/);
});

test("with no rules, the view shows how to add one", async () => {
  const none = { mode: "observe", version: null, rules: [], error: null, file: { state: "missing" } };
  const t = text(await render("/src/views/Rules.svelte", { policy: none }));
  assert.match(t, /No rules yet/);
  assert.match(t, /effect = "bundle"/);
});

test("the brief shows a notice when the policy file waits or has an error", async () => {
  const notice = (policy) => render("/src/views/PolicyNotice.svelte", { policy }).then(text);
  assert.match(await notice(fx.policy), /Your policy file changed\. .* Review the change/);
  assert.match(await notice({ ...same, file: { state: "invalid", error: { line: 4, message: "x" } } }), /error on line 4/);
  assert.match(await notice({ ...same, error: { line: 2, message: "x" } }), /Iris cannot use its rules/);
  assert.equal((await notice(same)).trim(), "");
});

test("a policy change is a POST with JSON and the token, and gives the API error text", async () => {
  const { post } = await load("/src/api.ts");
  const saved = globalThis.fetch;
  const calls = [];
  try {
    globalThis.fetch = async (url, init) => (calls.push({ url, init }), new Response(JSON.stringify({ mode: "apply" }), { status: 200 }));
    assert.deepEqual(await post("policy/mode", { version: 3, mode: "apply" }), { mode: "apply" });
    assert.equal(calls[0].url, "/api/policy/mode");
    assert.equal(calls[0].init.method, "POST");
    assert.equal(calls[0].init.headers["content-type"], "application/json");
    assert.deepEqual(JSON.parse(calls[0].init.body), { version: 3, mode: "apply" });
    globalThis.fetch = async () => new Response(JSON.stringify({ error: "The policy changed after you saw it. Look at it again." }), { status: 409 });
    await assert.rejects(post("policy/remove", { version: 3, id: 1 }), { message: "The policy changed after you saw it. Look at it again." });
  } finally {
    globalThis.fetch = saved;
  }
});

test("the rules have a route", async () => {
  const { parse } = await load("/src/route.ts");
  assert.deepEqual(parse("#/rules"), { view: "rules" });
});
