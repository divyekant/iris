// The proposal of a matter in the brief (R6-01, option A): the card, the hold
// with Undo, the confirm and decline requests, and the answers of the engine.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const view = fx.matterViews["g-5"];
const text = (el) => el.textContent.replace(/\s+/g, " ");
const button = (v, name) => [...v.target.querySelectorAll("button")].find((b) => text(b).trim() === name);

function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    asked.push([String(url).replace(/^\/api\//, ""), init?.body ? JSON.parse(init.body) : undefined]);
    const [status, out] = await answer();
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

test("the card shows the title, the counts and each member with a link", async () => {
  const v = await mount("/src/views/MatterCard.svelte", { view });
  try {
    const t = text(v.target);
    assert.match(t, /Iris sees a matter/);
    assert.match(t, new RegExp(view.matter.title));
    assert.match(t, /One record, one commitment/);
    for (const list of Object.values(view.items)) for (const item of list) assert.ok(v.target.querySelector(`a[href="#/item/${item.id}"]`), item.id);
    assert.ok(button(v, "Keep together") && button(v, "Not a matter"));
  } finally {
    v.destroy();
  }
});

test("Not a matter waits, then sends the decline and gives the answer", async () => {
  const asked = serve(() => [200, view]);
  let answer = null;
  const v = await mount("/src/views/MatterCard.svelte", { view, delay: 30, ondecided: (a) => (answer = a) });
  try {
    button(v, "Not a matter").click();
    await until(() => button(v, "Undo"));
    assert.equal(asked.length, 0, "no request during the hold");
    assert.match(text(v.target), /Not a matter\. Iris does not propose it again\./);
    await until(() => answer);
    assert.deepEqual(asked, [["matters/g-5/decline", {}]]);
    assert.equal(answer.matter.id, "g-5");
  } finally {
    v.destroy();
  }
});

test("Keep together opens the confirm step: the person changes the title and clears members", async () => {
  const asked = serve(() => [200, view]);
  let decided = 0;
  const v = await mount("/src/views/MatterCard.svelte", { view, delay: 30, ondecided: () => decided++ });
  try {
    button(v, "Keep together").click();
    v.flushSync();
    assert.equal(asked.length, 0, "the step sends nothing");
    const input = v.target.querySelector("input[type=text]");
    assert.equal(input.value, view.matter.title);
    const ticks = [...v.target.querySelectorAll("input[type=checkbox]")];
    assert.equal(ticks.length, view.matter.items.length);
    assert.ok(ticks.every((t) => t.checked));
    // Each member cleared: nothing to keep, so no confirm.
    for (const t of ticks) t.click();
    v.flushSync();
    assert.ok(v.target.querySelector("button.keep").disabled);
    ticks[0].click();
    v.flushSync();
    assert.equal(text(v.target.querySelector("button.keep")).trim(), "Keep 1 together");
    // An empty title: no confirm either.
    input.value = "   ";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    v.flushSync();
    assert.ok(v.target.querySelector("button.keep").disabled);
    input.value = "  New office  ";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    v.flushSync();
    v.target.querySelector("button.keep").click();
    await until(() => button(v, "Undo"));
    assert.equal(asked.length, 0, "no request during the hold");
    await until(() => decided);
    const kept = ticks[0].getAttribute("aria-label").replace(/^Keep /, "");
    const id = Object.values(view.items).flat().find((i) => i.title === kept).id;
    assert.deepEqual(asked, [["matters/g-5/confirm", { title: "New office", items: [id] }]]);
  } finally {
    v.destroy();
  }
});

test("Cancel leaves the confirm step with no request", async () => {
  const asked = serve(() => [200, view]);
  const v = await mount("/src/views/MatterCard.svelte", { view, delay: 30 });
  try {
    button(v, "Keep together").click();
    v.flushSync();
    button(v, "Cancel").click();
    v.flushSync();
    assert.ok(button(v, "Not a matter"));
    assert.equal(v.target.querySelector("input"), null);
    assert.equal(asked.length, 0);
  } finally {
    v.destroy();
  }
});

test("Undo sends nothing and shows the choices again", async () => {
  const asked = serve(() => [200, view]);
  const v = await mount("/src/views/MatterCard.svelte", { view, delay: 60 });
  try {
    button(v, "Not a matter").click();
    await until(() => button(v, "Undo"));
    button(v, "Undo").click();
    await until(() => button(v, "Keep together"));
    await new Promise((r) => setTimeout(r, 120));
    assert.equal(asked.length, 0);
  } finally {
    v.destroy();
  }
});

test("the answers of the engine: a changed proposal loads the next one, a lost answer says so", async () => {
  for (const [status, said, next] of [
    [409, /Iris did not record it/, 1],
    [404, /Iris did not record it/, 1],
    [400, /Iris did not record it/, 0],
    [500, /Iris does not know if it recorded the decision/, 0],
  ]) {
    serve(() => [status, { error: "no" }]);
    let decided = 0;
    const v = await mount("/src/views/MatterCard.svelte", { view, delay: 1, ondecided: () => decided++ });
    try {
      button(v, "Not a matter").click();
      await until(() => said.test(text(v.target)));
      assert.equal(decided, next, String(status));
    } finally {
      v.destroy();
    }
  }
});
