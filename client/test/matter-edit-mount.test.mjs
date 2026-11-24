// The edits of a confirmed matter (D19 of docs/specs/2026-11-24-matters-edit.md),
// the items that Iris proposes to add (D20), and the matter picker of an item.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const view = fx.matterViews["g-4"];
const text = (el) => el.textContent.replace(/\s+/g, " ");
const button = (v, name) => [...v.target.querySelectorAll("button")].find((b) => text(b).trim() === name);

function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push([path, init?.body ? JSON.parse(init.body) : undefined]);
    const [status, out] = await answer(path, init?.method ?? "GET");
    return new Response(status === 204 ? null : JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

test("Edit sends the new title and the removed members in one request, then shows the answer", async () => {
  const after = { ...view, matter: { ...view.matter, title: "Tokyo, March" } };
  const asked = serve(() => [200, after]);
  const v = await mount("/src/views/Matter.svelte", { view });
  try {
    button(v, "Edit").click();
    v.flushSync();
    assert.equal(asked.length, 0, "Edit sends nothing");
    const save = () => button(v, "Save");
    assert.ok(save().disabled, "no change, no Save");
    const input = v.target.querySelector("input[type=text]");
    input.value = "  Tokyo, March  ";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    const signal = view.items.signal[0];
    v.target.querySelector(`button[aria-label="Remove ${signal.title}"]`).click();
    v.flushSync();
    assert.ok(!save().disabled);
    save().click();
    await until(() => !v.target.querySelector("input[type=text]"));
    assert.deepEqual(asked, [["matters/g-4/edit", { title: "Tokyo, March", remove: [signal.id] }]]);
    assert.match(text(v.target), /Tokyo, March/);
  } finally {
    v.destroy();
  }
});

test("Save stays off when each member is out, and Delete asks once before the request", async () => {
  const asked = serve(() => [204, null]);
  globalThis.location = window.location;
  location.hash = "#/matters/g-4";
  const v = await mount("/src/views/Matter.svelte", { view });
  try {
    button(v, "Edit").click();
    v.flushSync();
    for (const b of [...v.target.querySelectorAll("button")].filter((b) => text(b).trim() === "Remove")) b.click();
    v.flushSync();
    assert.ok(button(v, "Save").disabled);
    assert.match(text(v.target), /Delete it instead/);
    button(v, "Delete matter").click();
    v.flushSync();
    assert.equal(asked.length, 0, "the first press only asks");
    assert.match(text(v.target), /Delete this matter\? Its items stay in Iris/);
    button(v, "Delete matter").click();
    await until(() => location.hash === "#/matters");
    assert.deepEqual(asked, [["matters/g-4/delete", {}]]);
  } finally {
    v.destroy();
    location.hash = "";
    delete globalThis.location;
  }
});

test("Add and Not this send one edit each; a lost answer says that Iris does not know", async () => {
  const added = view.additions[0];
  let status = 200;
  const asked = serve(() => [status, { ...view, additions: [], matter: { ...view.matter, additions: undefined } }]);
  const v = await mount("/src/views/Matter.svelte", { view });
  try {
    assert.match(text(v.target), /Iris thinks this belongs here/);
    status = 502;
    button(v, "Not this").click();
    await until(() => v.target.querySelector("[role=alert]"));
    assert.match(text(v.target), /does not know if it saved the change/);
    status = 200;
    button(v, "Add").click();
    await until(() => !/Iris thinks/.test(text(v.target)));
    assert.deepEqual(asked, [
      ["matters/g-4/edit", { remove: [added.id] }],
      ["matters/g-4/edit", { add: [added.id] }],
    ]);
  } finally {
    v.destroy();
  }
});

test("a matter that the person made says so, and a proposal has no Edit", async () => {
  const mine = fx.matterViews["g-9"];
  let v = await mount("/src/views/Matter.svelte", { view: mine });
  try {
    assert.match(text(v.target), /You made it/);
    assert.ok(button(v, "Edit"));
  } finally {
    v.destroy();
  }
  v = await mount("/src/views/Matter.svelte", { view: fx.matterViews["g-5"] });
  try {
    assert.ok(!button(v, "Edit"));
  } finally {
    v.destroy();
  }
});

test("the picker of an item adds it to a confirmed matter or makes a new matter", async () => {
  const item = fx.items.find((i) => i.form === "commitment" && !i.matter);
  const made = { ...view, matter: { ...view.matter, id: "g-10", title: "Rent" } };
  const asked = serve((path, method) =>
    path.endsWith("/joinable") ? [200, { joinable: true }] : method === "GET" ? [200, fx.matters] : [200, made],
  );
  let joined = null;
  const v = await mount("/src/views/MatterPick.svelte", { item, onchange: (m) => (joined = m) });
  try {
    await until(() => button(v, "Add to a matter"));
    button(v, "Add to a matter").click();
    await until(() => button(v, "Tokyo trip"));
    assert.ok(!button(v, fx.matterViews["g-5"].matter.title), "no proposal in the list");
    const input = v.target.querySelector("input[type=text]");
    input.value = "  Rent ";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    v.flushSync();
    v.target.querySelector("form").dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await until(() => joined);
    assert.equal(joined, "g-10");
    assert.deepEqual(asked.slice(2), [["matters", { title: "Rent", items: [item.id] }]]);
  } finally {
    v.destroy();
  }
});

test("a feed item has no picker", async () => {
  const item = fx.items.find((i) => i.form === "feed");
  const v = await mount("/src/views/MatterPick.svelte", { item, onchange: () => {} });
  try {
    assert.equal(text(v.target).trim(), "");
  } finally {
    v.destroy();
  }
});

test("the picker shows only when the engine says that the item can join a matter", async () => {
  const item = fx.items.find((i) => i.form === "signal" && !i.matter) ?? fx.items.find((i) => i.form === "commitment" && !i.matter);
  for (const joinable of [true, false]) {
    const asked = serve(() => [200, { joinable }]);
    const v = await mount("/src/views/MatterPick.svelte", { item, onchange: () => {} });
    try {
      await until(() => asked.length === 1);
      await new Promise((r) => setTimeout(r, 0));
      v.flushSync();
      assert.deepEqual(asked[0], [`items/${encodeURIComponent(item.id)}/joinable`, undefined]);
      assert.equal(Boolean(button(v, "Add to a matter")), joinable);
    } finally {
      v.destroy();
    }
  }
});

test("a lost answer of a write that committed shows the matter; a refusal says that Iris did not add it", async () => {
  const item = fx.items.find((i) => i.form === "commitment" && !i.matter);
  for (const [committed, answer, says] of [
    [true, 502, null],
    [true, 409, null],
    [false, 400, /Iris did not add it/],
    [false, 503, /does not know if it added/],
  ]) {
    serve((path, method) => {
      if (path.endsWith("/joinable")) return [200, { joinable: true }];
      if (path === "matters" && method === "GET") return [200, fx.matters];
      if (method === "POST") return [answer, { error: "no" }];
      return [200, { ...item, matter: committed ? "g-4" : undefined }];
    });
    let joined = null;
    const v = await mount("/src/views/MatterPick.svelte", { item, onchange: (m) => (joined = m) });
    try {
      await until(() => button(v, "Add to a matter"));
      button(v, "Add to a matter").click();
      await until(() => button(v, "Tokyo trip"));
      button(v, "Tokyo trip").click();
      if (says) {
        await until(() => v.target.querySelector("[role=alert]"));
        assert.match(text(v.target.querySelector("[role=alert]")), says);
        assert.equal(joined, null);
      } else {
        await until(() => joined);
        assert.equal(joined, "g-4", `answer ${answer}`);
        assert.equal(v.target.querySelector("[role=alert]"), null);
      }
    } finally {
      v.destroy();
    }
  }
});

test("after another tab removes a marked member, Save sends only the title", async () => {
  const signal = view.items.signal[0];
  const fresh = { ...view, items: { ...view.items, signal: view.items.signal.slice(1) } };
  const saved = { ...fresh, matter: { ...fresh.matter, title: "Tokyo, March" } };
  let posts = 0;
  const asked = serve((path, method) => {
    if (method === "GET") return [200, fresh];
    posts += 1;
    return posts === 1 ? [409, { error: "an item to remove is not a member" }] : [200, saved];
  });
  const v = await mount("/src/views/Matter.svelte", { view });
  try {
    button(v, "Edit").click();
    v.flushSync();
    const input = v.target.querySelector("input[type=text]");
    input.value = "Tokyo, March";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    v.target.querySelector(`button[aria-label="Remove ${signal.title}"]`).click();
    v.flushSync();
    button(v, "Save").click();
    await until(() => asked.some(([p, b]) => p === `matters/${view.matter.id}` && b === undefined));
    await until(() => !button(v, "Save").disabled);
    button(v, "Save").click();
    await until(() => posts === 2);
    assert.deepEqual(asked.at(-1)[1], { title: "Tokyo, March" });
  } finally {
    v.destroy();
  }
});
