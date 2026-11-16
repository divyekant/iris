// Done, Later and Decline on a commitment in a DOM: the hold with Undo, the
// request, and the answers of the engine (spec 2 section 4).
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const act = await load("/src/act.ts");
const NOW = new Date(2026, 10, 16, 9, 0); // A Monday.
const commitment = (status = "open") => ({ ...fx.items.find((i) => i.form === "commitment"), status });
const text = (el) => el.textContent.replace(/\s+/g, " ");

// Answer each POST with `answer(path, body)`: [status, body], or throw for no answer.
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

const button = (view, name) => [...view.target.querySelectorAll("button")].find((b) => text(b).trim().startsWith(name));

test("the days of Later", () => {
  assert.equal(act.tomorrow(NOW), "2026-11-17");
  assert.equal(act.nextWeek(NOW), "2026-11-23", "on a Monday, the Monday a week later");
  assert.equal(act.nextWeek(new Date(2026, 10, 22)), "2026-11-23", "on a Sunday, the next day");
  assert.equal(act.tomorrow(new Date(2026, 11, 31)), "2027-01-01");
  assert.equal(act.said("defer", "2026-11-23"), "Later, on Mon 23 Nov.");
});

test("Done waits, then sends close and shows the new item", async () => {
  const asked = serve((path) => [200, { ...commitment("done") }]);
  let changed = null;
  const view = await mount("/src/views/Act.svelte", { item: commitment(), now: NOW, delay: 30, onchange: (i) => (changed = i) });
  try {
    button(view, "Done").click();
    await until(() => text(view.target).includes("Undo"));
    assert.equal(asked.length, 0, "no request during the hold");
    await until(() => changed);
    assert.deepEqual(asked, [["items/c-311/close", {}]]);
    assert.equal(changed.status, "done");
    assert.match(text(view.target), /Done\. Back to the brief/);
  } finally {
    view.destroy();
  }
});

test("Undo in the hold sends nothing", async () => {
  const asked = serve(() => [200, commitment("done")]);
  const view = await mount("/src/views/Act.svelte", { item: commitment(), now: NOW, delay: 200, onchange: () => {} });
  try {
    button(view, "Decline").click();
    await until(() => button(view, "Undo"));
    button(view, "Undo").click();
    await new Promise((r) => setTimeout(r, 300));
    assert.equal(asked.length, 0);
    assert.ok(button(view, "Done"), "the actions come back");
  } finally {
    view.destroy();
  }
});

test("Later sends defer with the day, and leaving in the hold still sends", async () => {
  const asked = serve(() => [200, commitment("deferred")]);
  const view = await mount("/src/views/Act.svelte", { item: commitment(), now: NOW, delay: 60_000, onchange: () => {} });
  button(view, "Later").click();
  await until(() => button(view, "Next week"));
  button(view, "Next week").click();
  await until(() => button(view, "Undo"));
  view.destroy();
  await until(() => asked.length === 1);
  assert.deepEqual(asked, [["items/c-311/defer", { date: "2026-11-23" }]]);
});

test("a refusal says why, and no answer says Iris does not know", async () => {
  for (const [answer, want] of [
    [() => [409, { error: "c-311 is done: close changes an open or deferred commitment" }], /did not record it: c-311 is done/],
    [() => { throw new TypeError("network"); }, /does not know if it recorded the change/],
  ]) {
    serve(answer);
    const view = await mount("/src/views/Act.svelte", { item: commitment(), now: NOW, delay: 10, onchange: () => {} });
    try {
      button(view, "Done").click();
      await until(() => view.target.querySelector(".error"));
      assert.match(text(view.target), want);
    } finally {
      view.destroy();
    }
  }
});

test("only an open or deferred commitment has the actions", async () => {
  for (const [item, has] of [[commitment("deferred"), true], [commitment("done"), false], [commitment("declined"), false], [fx.items.find((i) => i.form === "record"), false]]) {
    const view = await mount("/src/views/Act.svelte", { item, now: NOW, onchange: () => {} });
    assert.equal(Boolean(button(view, "Done")), has, `${item.id} ${item.status}`);
    view.destroy();
  }
});
