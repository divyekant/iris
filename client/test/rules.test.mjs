// The policy view (R2-08): words for rules, each state of the file, and the POST of a change.
import { mock, test } from "node:test";
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
  assert.match(t, /New Mark receipts as read Allowed Mark as read Records Receipts/);
  assert.match(t, /Changed Archive newsletters after 5 days .* Before: Archive newsletters after 3 days, Archive and mark as read, Feed, From services, Older than 3 days/);
  assert.match(t, /Confirm the change Discard/);
  assert.match(t, /Confirm or discard the change of your file first/);
  // Each mode button and each remove button waits.
  assert.equal(html.match(/name="mode"[^>]*disabled|disabled[^>]*name="mode"/g)?.length, 2);
  assert.equal(html.match(/class="quiet remove[^"]*"[^>]*disabled/g)?.length, fx.policy.rules.length);
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
  const none = { mode: "observe", version: null, next_id: 1, rules: [], error: null, file: { state: "missing" } };
  const t = text(await render("/src/views/Rules.svelte", { policy: none }));
  assert.match(t, /No rules yet/);
  assert.match(t, /Make policy\.toml/);
  // At the first start the example is a complete file: the parser needs version and mode.
  assert.match(t, /version = 0 mode = "observe" \[\[rule\]\] id = 1 .* effect = "bundle"/);
  // After the last rule went, the example takes an ID that no rule had.
  const empty = { ...none, version: 4, next_id: 3, file: { state: "same" } };
  const u = text(await render("/src/views/Rules.svelte", { policy: empty }));
  assert.match(u, /Add a rule to the end of policy\.toml .* \[\[rule\]\] id = 3 /);
  assert.doesNotMatch(u, /version = 0/);
  assert.doesNotMatch(await render("/src/views/Rules.svelte", { policy: none }), /disabled/);
});

test("a confirmed file that is gone blocks the changes from here, and the view can put it back", async () => {
  const gone = { ...same, file: { state: "missing" } };
  const html = await render("/src/views/Rules.svelte", { policy: gone });
  const t = text(html);
  assert.match(t, /Your policy file is missing .* Put the file back/);
  assert.match(t, /Put your policy file back first/);
  assert.equal(html.match(/class="quiet remove[^"]*"[^>]*disabled/g)?.length, fx.policy.rules.length);
  assert.match(text(await render("/src/views/PolicyNotice.svelte", { policy: gone })), /Your policy file is missing/);
});

test("the difference names the effect of each rule, also when only the effect changed", async () => {
  const [rule] = fx.policy.rules;
  const deny = { ...rule, effect: "deny" };
  const file = { ...fx.policy.file, mode: same.mode, added: [{ ...rule, id: 9, effect: "allow" }], removed: [deny], changed: [{ before: deny, after: rule }] };
  const t = text(await render("/src/views/Rules.svelte", { policy: { ...same, file } }));
  assert.match(t, new RegExp(`New ${rule.name} Allowed`));
  assert.match(t, new RegExp(`Changed ${rule.name} Allowed .* Before: Never`));
  assert.match(t, new RegExp(`Removed ${rule.name} Never`));
});

test("the difference shows a change of the author or the date of a rule", async () => {
  const [rule] = fx.policy.rules;
  const after = { ...rule, by: "proposal", added: "2026-11-12" };
  const file = { ...fx.policy.file, mode: same.mode, added: [], removed: [], changed: [{ before: rule, after }] };
  const t = text(await render("/src/views/Rules.svelte", { policy: { ...same, file } }));
  assert.match(t, new RegExp(`Changed ${rule.name} Allowed .* Iris proposed it Added 2026-11-12 Before: You wrote it, Added ${rule.added}, Archive`));
});

test("the apply mode says what Iris does not do", async () => {
  const t = text(await render("/src/views/Rules.svelte", { policy: same }));
  assert.match(t, /Apply Iris makes the changes that your rules allow\. It never makes a change that a Never rule or a safety limit forbids\. All other changes wait for you\./);
});

test("a new load of the policy keeps a mode that the person chose and did not send", async () => {
  const { keep } = await load("/src/rules.ts");
  assert.equal(keep("observe", "observe", "apply"), "apply");
  assert.equal(keep("apply", "observe", "observe"), "apply");
  assert.equal(keep("apply", "observe", "apply"), "apply");
});

test("the notice above the brief looks at the policy again while it shows", async () => {
  const { follow } = await load("/src/clock.ts");
  mock.timers.enable({ apis: ["setTimeout"] });
  const flush = () => new Promise((r) => setImmediate(r));
  try {
    const answers = [Promise.resolve("same"), Promise.resolve("invalid")];
    const seen = [];
    const stop = follow(() => answers.shift() ?? Promise.resolve("stop"), (v) => seen.push(v), 30000);
    await flush();
    assert.deepEqual(seen, ["same"]);
    mock.timers.tick(30000);
    await flush();
    assert.deepEqual(seen, ["same", "invalid"]);
    stop();
    mock.timers.tick(60000);
    await flush();
    assert.deepEqual(seen, ["same", "invalid"]);
  } finally {
    mock.timers.reset();
  }
});

test("the brief shows a notice when the policy file waits or has an error", async () => {
  const notice = (policy) => render("/src/views/PolicyNotice.svelte", { policy }).then(text);
  assert.match(await notice(fx.policy), /Your policy file changed\. .* Review the change/);
  assert.match(await notice({ ...same, file: { state: "invalid", error: { line: 4, message: "x" } } }), /error on line 4/);
  assert.match(await notice({ ...same, error: { line: 2, message: "x" } }), /Iris cannot use its rules/);
  assert.equal((await notice({ ...same, proposals: [] })).trim(), "");
});

test("the suggestions of Iris show above the mode, and wait while the file waits", async () => {
  const t = text(await render("/src/views/Rules.svelte", { policy: same }));
  assert.ok(t.indexOf("Suggested by Iris") < t.indexOf("Mode"), t);
  assert.match(t, /Archive mail from deals@acme\.example Archive From deals@acme\.example You approved all 14 archive changes like this\. Add the rule Dismiss/);
  assert.match(t, /Mark mail from services as read Mark as read From services You approved all 11 read changes like this\./);
  assert.match(t, /Hide feed items from digest@news\.example .*You hid all 12 items like this\./);
  const blocked = await render("/src/views/Rules.svelte", { policy: fx.policy });
  // Add waits for the file; Dismiss changes no file.
  assert.equal(blocked.match(/class="accept[^"]*"[^>]*disabled/g)?.length, 3);
  assert.doesNotMatch(text(await render("/src/views/Rules.svelte", { policy: { ...same, proposals: [] } })), /Suggested by Iris/);
});

test("the brief says that Iris suggests rules, after each notice of the file", async () => {
  const notice = (policy) => render("/src/views/PolicyNotice.svelte", { policy }).then(text);
  assert.match(await notice(same), /Iris suggests 3 rules from what you approve and hide\. See the suggestions/);
  assert.match(await notice({ ...same, proposals: [same.proposals[0]] }), /Iris suggests a rule from your approvals\. See the suggestions/);
  assert.match(await notice({ ...same, proposals: [same.proposals[2]] }), /Iris suggests a rule from what you hide\. See the suggestions/);
  assert.doesNotMatch(await notice(fx.policy), /suggests/);
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
