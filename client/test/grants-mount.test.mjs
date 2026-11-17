// The grants inside one job (R4-10) in a DOM: the switches, the body of each
// change, a grant from the file, and a job that ended.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const s = await load("/src/start.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");
const policy = (rules) => ({ ...fx.policy, mode: "apply", version: 7, rules });
const grant = (id, verbs, job = "j-12") => ({ id, name: `g${id}`, effect: "allow", verbs, when: { author: "agent" }, by: "person", added: "2026-11-17", job, line: 1 });
const job = (state = "working") => ({ ...fx.jobs["j-12"], state });
const switches = (view) => [...view.target.querySelectorAll("input[role=switch]")];

function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    const body = init?.body ? JSON.parse(init.body) : undefined;
    asked.push([path, body]);
    const [status, out] = await answer(path, body);
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

test("ruleOf matches a grant with the same verbs in any order, and grantsOf only the allow rules of the job", () => {
  const rules = [grant(1, ["trash"]), grant(2, ["decline", "close", "defer"]), grant(3, ["trash"], "j-1"), { ...grant(4, ["archive"]), effect: "review" }];
  const mine = s.grantsOf(rules, "j-12");
  assert.deepEqual(mine.map((r) => r.id), [1, 2]);
  assert.equal(s.ruleOf(mine, s.GRANTS[1])?.id, 2);
  assert.equal(s.ruleOf(mine, s.GRANTS[2])?.id, 1);
  assert.equal(s.ruleOf(mine, s.GRANTS[0]), undefined);
  // Each verb of a group is a verb that a rule can allow, and no verb is in two groups.
  const all = s.GRANTS.flatMap((g) => g.verbs);
  assert.equal(new Set(all).size, all.length);
  for (const v of all) assert.ok(!["draft", "send"].includes(v));
});

test("a switch adds the grant of its group, with the version that the page saw; off removes the rule", async () => {
  const after = policy([grant(1, ["trash"]), grant(20, [...s.GRANTS[0].verbs])]);
  const asked = serve((path) => [200, path === "policy/grant" ? after : policy([])]);
  const view = await mount("/src/views/Grants.svelte", { job: job(), policy: policy([grant(1, ["trash"])]) });
  assert.deepEqual(switches(view).map((x) => x.checked), [false, false, true]);
  switches(view)[0].click();
  await until(() => asked.length === 1);
  assert.deepEqual(asked[0], ["policy/grant", { version: 7, job: "j-12", verbs: [...s.GRANTS[0].verbs] }]);
  await until(() => (view.flushSync(), switches(view)[0].checked));
  switches(view)[2].click();
  await until(() => asked.length === 2);
  assert.deepEqual(asked[1], ["policy/remove", { version: 7, id: 1 }]);
  view.destroy();
});

test("a grant with other verbs shows as its own line with Remove; a refusal shows the reason", async () => {
  serve(() => [409, { error: "The policy changed. Load it again." }]);
  const view = await mount("/src/views/Grants.svelte", { job: job(), policy: policy([grant(5, ["archive"]), grant(6, ["trash"], "j-1")]) });
  assert.match(text(view.target), /From your policy file, rule 5/);
  assert.doesNotMatch(text(view.target), /rule 6/, "a grant of another job does not show");
  [...view.target.querySelectorAll("button")].find((b) => text(b).includes("Remove")).click();
  await until(() => view.target.querySelector("[role=alert]"));
  assert.match(text(view.target.querySelector("[role=alert]")), /The policy changed/);
  view.destroy();
});

test("a job that ended adds no grant, but its grants can still go; observe mode says that nothing applies", async () => {
  serve(() => [200, policy([])]);
  const view = await mount("/src/views/Grants.svelte", { job: job("done"), policy: { ...policy([grant(1, ["trash"])]), mode: "observe" } });
  assert.deepEqual(switches(view).map((x) => x.disabled), [true, true, false]);
  assert.match(text(view.target), /The job ended, so these grants allow nothing more\./);
  assert.match(text(view.target), /observe mode/);
  view.destroy();
});

test("the job page shows the grants when the policy loaded, and no grants without it", async () => {
  serve(() => [200, policy([])]);
  for (const [p, shows] of [[policy([]), true], [null, false]]) {
    const view = await mount("/src/views/Job.svelte", { detail: fx.jobDetails["j-12"], policy: p });
    assert.equal(/Without asking you/.test(text(view.target)), shows);
    view.destroy();
  }
});
