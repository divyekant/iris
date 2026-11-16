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

test("leaving in the hold still sends", async () => {
  const asked = serve(() => [200, commitment("done")]);
  const view = await mount("/src/views/Line.svelte", { item: commitment(), delay: 60_000, ondone: () => {} });
  box(view).click();
  await until(() => button(view, "Undo"));
  view.destroy();
  await until(() => asked.length === 1);
  assert.deepEqual(asked, [["items/c-311/close", {}]]);
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
