// The suggestions of the Rules screen in a DOM (R5-01): Add and Dismiss send their POST, and the answer replaces the view.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const same = { ...fx.policy, file: { state: "same" } };
const text = (el) => el.textContent.replace(/\s+/g, " ");

function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    asked.push([String(url), init?.body ? JSON.parse(init.body) : null]);
    const [status, body] = answer(String(url));
    return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

const button = (view, label, n = 0) => [...view.target.querySelectorAll(".proposal button")].filter((b) => b.textContent.trim() === label)[n];

test("Add the rule sends the version and the proposal ID, and the new policy has no suggestion", async () => {
  const added = { ...same, version: 4, proposals: [same.proposals[1]], rules: [...same.rules, { ...same.proposals[0].rule, line: 40 }] };
  const asked = serve(() => [200, added]);
  const view = await mount("/src/views/Rules.svelte", { policy: same, every: 0 });
  try {
    assert.equal(view.target.querySelectorAll(".proposal").length, 2);
    button(view, "Add the rule").click();
    await until(() => view.target.querySelectorAll(".proposal").length === 1);
    assert.deepEqual(asked, [["/api/policy/accept", { version: 3, id: 1 }]]);
    assert.match(text(view.target), /Archive mail from deals@acme\.example .*Rule 6 · line 40 · Iris proposed it/);
  } finally {
    view.destroy();
  }
});

test("Dismiss sends the proposal ID, and a 409 shows the error and loads the policy again", async () => {
  const asked = serve((url) => (url.endsWith("dismiss") ? [409, { error: "The proposal is not open any more." }] : [200, { ...same, proposals: [] }]));
  const view = await mount("/src/views/Rules.svelte", { policy: same, every: 0 });
  try {
    button(view, "Dismiss", 1).click();
    await until(() => view.target.querySelector('[role="alert"]'));
    assert.match(text(view.target), /The proposal is not open any more\./);
    assert.deepEqual(asked[0], ["/api/policy/dismiss", { id: 2 }]);
    assert.equal(asked[1][0], "/api/policy");
    assert.equal(view.target.querySelectorAll(".proposal").length, 0);
  } finally {
    view.destroy();
  }
});
