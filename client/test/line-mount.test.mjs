// The quick Done on a brief line (card R2-16): the tick box, the hold with
// Undo, the close request and the answers of the engine.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const commitment = (status = "open") => ({ ...fx.items.find((i) => i.form === "commitment"), status });
const text = (el) => el.textContent.replace(/\s+/g, " ");
const box = (view) => view.target.querySelector("button.box");
const button = (view, name) => [...view.target.querySelectorAll("button")].find((b) => text(b).trim() === name);

function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    asked.push([String(url).replace(/^\/api\//, ""), init?.body ? JSON.parse(init.body) : undefined]);
    const [status, out] = await answer();
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

test("the tick box shows only on an open or deferred commitment with ondone", async () => {
  const cases = [
    [{ item: commitment(), ondone: () => {} }, true],
    [{ item: commitment("deferred"), ondone: () => {} }, true],
    [{ item: commitment("done"), ondone: () => {} }, false],
    [{ item: commitment("declined"), ondone: () => {} }, false],
    [{ item: commitment() }, false],
    [{ item: fx.items.find((i) => i.form === "conversation"), ondone: () => {} }, false],
  ];
  for (const [props, shown] of cases) {
    const view = await mount("/src/views/Line.svelte", props);
    try {
      assert.equal(!!box(view), shown, `${props.item.form} ${props.item.status} ondone=${!!props.ondone}`);
      if (shown) assert.equal(box(view).getAttribute("aria-label"), `Done: ${props.item.title}`);
      else assert.ok(view.target.querySelector(".mark"), "the initials stay");
    } finally {
      view.destroy();
    }
  }
});

test("a tick waits, then sends close and calls ondone", async () => {
  const asked = serve(() => [200, commitment("done")]);
  let done = null;
  const view = await mount("/src/views/Line.svelte", { item: commitment(), delay: 30, ondone: (id) => (done = id) });
  try {
    box(view).click();
    await until(() => button(view, "Undo"));
    assert.equal(asked.length, 0, "no request during the hold");
    assert.match(text(view.target), /Done\. It leaves your brief\./);
    await until(() => done);
    assert.deepEqual(asked, [["items/c-311/close", {}]]);
    assert.equal(done, "c-311");
  } finally {
    view.destroy();
  }
});

test("Undo sends nothing and brings the tick box back", async () => {
  const asked = serve(() => [200, commitment("done")]);
  const view = await mount("/src/views/Line.svelte", { item: commitment(), delay: 200, ondone: () => {} });
  try {
    box(view).click();
    await until(() => button(view, "Undo"));
    button(view, "Undo").click();
    await new Promise((r) => setTimeout(r, 300));
    assert.equal(asked.length, 0);
    assert.ok(box(view));
  } finally {
    view.destroy();
  }
});

test("a row that leaves keeps its hold, and flushHolds sends at once", async () => {
  const act = await load("/src/act.ts");
  const asked = serve(() => [200, commitment("done")]);
  // A brief refresh removes the row: Undo time remains, so nothing goes yet.
  const row = await mount("/src/views/Line.svelte", { item: commitment(), delay: 150, ondone: () => {} });
  box(row).click();
  await until(() => button(row, "Undo"));
  row.destroy();
  await new Promise((r) => setTimeout(r, 50));
  assert.equal(asked.length, 0, "no send when one row goes");
  await until(() => asked.length === 1);
  // The person leaves the view: each hold sends now.
  const left = await mount("/src/views/Line.svelte", { item: commitment(), delay: 60_000, ondone: () => {} });
  box(left).click();
  await until(() => button(left, "Undo"));
  act.flushHolds();
  await until(() => asked.length === 2);
  left.destroy();
  assert.deepEqual(asked[1], ["items/c-311/close", {}]);
});

const copy = (x) => JSON.parse(JSON.stringify(x));
const closed = () => {
  const b = copy(fx.brief);
  b.you_owe = { ...b.you_owe, count: b.you_owe.count - 1, items: b.you_owe.items.filter((i) => i.id !== "c-311") };
  return b;
};
const title = commitment().title;
const owes = (view) => [...view.target.querySelectorAll(".line")].some((l) => text(l).includes(title) && !l.classList.contains("said"));

test("the brief sends a held Done when the person leaves it", async () => {
  const asked = [];
  globalThis.fetch = async (url) => {
    asked.push(String(url));
    return new Response(JSON.stringify(commitment("done")), { status: 200, headers: { "content-type": "application/json" } });
  };
  const view = await mount("/src/views/Brief.svelte", { brief: copy(fx.brief), status: fx.status, watches: [], poll: 0, hold: 60_000 });
  box(view).click();
  await until(() => button(view, "Undo"));
  view.destroy();
  await until(() => asked.some((u) => u.endsWith("items/c-311/close")));
});

test("a page close in the hold sends at once, with keepalive", async () => {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    asked.push([String(url), init?.keepalive]);
    return new Response(JSON.stringify(commitment("done")), { status: 200, headers: { "content-type": "application/json" } });
  };
  const view = await mount("/src/views/Brief.svelte", { brief: copy(fx.brief), status: fx.status, watches: [], poll: 0, hold: 60_000 });
  try {
    box(view).click();
    await until(() => button(view, "Undo"));
    window.dispatchEvent(new Event("pagehide"));
    await until(() => asked.some(([u]) => u.endsWith("/close")));
    assert.equal(asked.find(([u]) => u.endsWith("/close"))[1], true, "keepalive");
  } finally {
    view.destroy();
  }
});

test("a poll that started before a quick Done does not bring the item back", async () => {
  let briefs = 0;
  let staleSent = false;
  globalThis.fetch = async (url) => {
    const path = String(url).replace(/^\/api\//, "");
    let out = {};
    if (path === "brief") {
      briefs += 1;
      if (briefs === 1) {
        // The first poll reads before the close and answers after the reload.
        await new Promise((r) => setTimeout(r, 300));
        staleSent = true;
        out = copy(fx.brief);
      } else out = closed();
    } else if (path === "status") out = fx.status;
    else if (path.startsWith("brief/signals")) out = { items: [], next: null };
    else if (path === "watches") out = [];
    else out = commitment("done");
    return new Response(JSON.stringify(out), { status: 200, headers: { "content-type": "application/json" } });
  };
  const view = await mount("/src/views/Brief.svelte", { brief: copy(fx.brief), status: { ...fx.status }, watches: [], poll: 20, hold: 10 });
  try {
    await until(() => briefs === 1);
    box(view).click();
    await until(() => briefs >= 2 && !owes(view));
    await until(() => staleSent);
    await new Promise((r) => setTimeout(r, 20));
    view.flushSync();
    assert.ok(!owes(view), "the closed item stays gone");
  } finally {
    view.destroy();
  }
});

test("a refusal shows the reason, a 409 loads the brief again, and no answer says so", async () => {
  for (const [status, reload, pattern] of [
    [400, false, /Iris did not record it: the item is closed/],
    [409, true, /Iris did not record it: the item is closed/],
    [500, false, /Iris does not know if it recorded the change/],
  ]) {
    serve(() => [status, { error: "the item is closed" }]);
    let called = false;
    const view = await mount("/src/views/Line.svelte", { item: commitment(), delay: 10, ondone: () => (called = true) });
    try {
      box(view).click();
      await until(() => view.target.querySelector(".error"));
      assert.match(text(view.target), pattern, String(status));
      assert.equal(called, reload, `${status}: ondone`);
      if (status < 500) {
        button(view, "Close").click();
        await until(() => box(view));
      }
    } finally {
      view.destroy();
    }
  }
});
