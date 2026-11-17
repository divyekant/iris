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

test("grantView: a switch takes only its exact rule; each other grant of the job is its own line; general allows that reach an agent", () => {
  const rules = [
    grant(1, ["trash"]),
    grant(2, ["decline", "close", "defer"]),
    grant(3, ["trash"], "j-1"),
    { ...grant(4, ["archive"]), effect: "review" },
    { ...grant(5, ["trash"]), when: { author: "agent", sender: "@shop.example" } },
    grant(6, ["trash"]),
    { ...grant(7, ["archive"]), job: null, when: { sender_kind: "service" } },
    { ...grant(8, ["read"]), job: null },
    { ...grant(9, ["archive"]), job: null, when: { author: "person" } },
  ];
  assert.deepEqual(s.grantsOf(rules, "j-12").map((r) => r.id), [1, 2, 5, 6]);
  const v = s.grantView(rules, "j-12");
  assert.equal(v.switches.trash?.id, 1);
  assert.equal(v.switches.commitments?.id, 2);
  assert.equal(v.switches.tidy, undefined);
  assert.deepEqual(v.others.map((r) => r.id), [5, 6], "a conditioned grant and a second copy show as their own lines");
  assert.deepEqual(v.general.map((r) => r.id), [7, 8], "no job, and no author or the author agent");
  assert.ok(!s.isSwitch({ ...grant(1, ["trash"]), when: {} }, s.GRANTS[2]), "a rule with no author condition is not the rule of a switch");
  // Each verb of a group is a verb that a rule can allow, and no verb is in two groups.
  const all = s.GRANTS.flatMap((g) => g.verbs);
  assert.equal(new Set(all).size, all.length);
  for (const v of all) assert.ok(!["draft", "send"].includes(v));
});

test("a switch adds the grant of its group, with the version that the page saw; off removes the rule", async () => {
  const after = policy([grant(1, ["trash"]), grant(20, [...s.GRANTS[0].verbs])]);
  const asked = serve((path) => [200, path === "policy/grant" ? after : path === "policy" ? policy([grant(1, ["trash"])]) : policy([])]);
  const view = await mount("/src/views/Grants.svelte", { job: job() });
  await until(() => switches(view).length === 3);
  assert.deepEqual(switches(view).map((x) => x.checked), [false, false, true]);
  switches(view)[0].click();
  await until(() => asked.length === 2);
  assert.deepEqual(asked[1], ["policy/grant", { version: 7, job: "j-12", verbs: [...s.GRANTS[0].verbs] }]);
  await until(() => (view.flushSync(), switches(view)[0].checked));
  switches(view)[2].click();
  await until(() => asked.length === 3);
  assert.deepEqual(asked[2], ["policy/remove", { version: 7, id: 1 }]);
  view.destroy();
});

test("a refused switch shows the policy as it is, in both directions, and the page reads the policy again", async () => {
  const asked = serve((path) => (path === "policy" ? [200, { ...policy([grant(1, ["trash"])]), version: 7 + asked.length }] : [409, { error: "The policy changed. Load it again." }]));
  const view = await mount("/src/views/Grants.svelte", { job: job() });
  await until(() => switches(view).length === 3);
  for (const i of [0, 2]) {
    const was = switches(view)[i].checked;
    switches(view)[i].click();
    await until(() => view.target.querySelector("[role=alert]"));
    await until(() => (view.flushSync(), !switches(view)[i].disabled));
    assert.equal(switches(view)[i].checked, was, `switch ${i} stays as the policy is`);
  }
  assert.deepEqual(asked.map(([p]) => p), ["policy", "policy/grant", "policy", "policy/remove", "policy"]);
  assert.equal(asked[3][1].version, 7 + 3, "the second change sends the version of the policy read again (the third request)");
  view.destroy();
});

test("a conditioned grant shows as its own line with its condition and Remove; a refusal shows the reason", async () => {
  serve((path) => (path === "policy" ? [200, policy([{ ...grant(5, ["trash"]), when: { author: "agent", sender: "@shop.example" } }, grant(6, ["trash"], "j-1")])] : [409, { error: "The policy changed. Load it again." }]));
  const view = await mount("/src/views/Grants.svelte", { job: job() });
  await until(() => switches(view).length === 3);
  assert.deepEqual(switches(view).map((x) => x.checked), [false, false, false], "the trash switch is off: the grant has a condition");
  assert.match(text(view.target), /From @shop\.example\. From your policy file, rule 5/);
  assert.doesNotMatch(text(view.target), /rule 6/, "a grant of another job does not show");
  [...view.target.querySelectorAll("button")].find((b) => text(b).includes("Remove")).click();
  await until(() => view.target.querySelector("[role=alert]"));
  assert.match(text(view.target.querySelector("[role=alert]")), /The policy changed/);
  view.destroy();
});

test("an allow rule of the whole policy shows, and the page does not say that the agent asks before each change", async () => {
  serve(() => [200, policy([{ ...grant(7, ["archive"]), job: null, when: {} }])]);
  const view = await mount("/src/views/Grants.svelte", { job: job() });
  await until(() => switches(view).length === 3);
  assert.match(text(view.target), /Also allowed by your whole policy/);
  assert.match(text(view.target), /except for the changes that the rules of your whole policy below allow/);
  assert.match(text(view.target), /rule 7/);
  view.destroy();
});

test("a job that ended adds no grant, but its grants can still go; observe mode says that nothing applies", async () => {
  serve(() => [200, { ...policy([grant(1, ["trash"])]), mode: "observe" }]);
  const view = await mount("/src/views/Grants.svelte", { job: job("done") });
  await until(() => switches(view).length === 3);
  assert.deepEqual(switches(view).map((x) => x.disabled), [true, true, false]);
  assert.match(text(view.target), /The job ended, so these grants allow nothing more\./);
  assert.match(text(view.target), /observe mode/);
  view.destroy();
});

test("a failed read of the policy shows the reason and Try again, not an empty list of grants", async () => {
  let ok = false;
  serve(() => (ok ? [200, policy([grant(1, ["trash"])])] : [500, { error: "The store is busy." }]));
  const view = await mount("/src/views/Job.svelte", { detail: fx.jobDetails["j-12"] });
  await until(() => view.target.querySelector("[role=alert]"));
  assert.match(text(view.target), /Without asking you/);
  assert.match(text(view.target), /Iris cannot read your policy now.*The store is busy\./);
  assert.equal(switches(view).length, 0);
  ok = true;
  [...view.target.querySelectorAll("button")].find((b) => text(b).includes("Try again")).click();
  await until(() => switches(view).length === 3);
  assert.equal(view.target.querySelector("[role=alert]"), null);
  view.destroy();
});
