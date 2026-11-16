// The handled view (R2-09) in a DOM: each list loads its next page.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");

test("each list of the handled view loads its next page", async () => {
  const asked = [];
  const pages = {
    "handled?limit=50": { ...fx.decided, next: "7" },
    "handled?limit=50&after=7": { count: 1, next: null, actions: [{ ...fx.decided.actions[0], changeset: "cs-3", state: "approved" }] },
    "brief/hidden?limit=50": { count: 4, items: fx.hidden.slice(0, 2), next: "c1", duplicates: { [fx.hidden[0].id]: ["c-901"] } },
    "brief/hidden?limit=50&after=c1": { count: 4, items: fx.hidden.slice(2), next: null, duplicates: { [fx.hidden[3].id]: ["c-902"] } },
  };
  const saved = globalThis.fetch;
  globalThis.fetch = async (url) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push(path);
    const body = pages[path] ?? fx.answer(`/${path}`);
    return new Response(JSON.stringify(body ?? { error: "Not found." }), { status: body === undefined ? 404 : 200, headers: { "content-type": "application/json" } });
  };
  const app = await mount("/src/views/Handled.svelte", { changes: pages["handled?limit=50"], hidden: pages["brief/hidden?limit=50"] });
  try {
    assert.match(text(app.target), /Archive 500 messages Rule 5 \(Archive old newsletters\) allows archive\./);
    const buttons = () => [...app.target.querySelectorAll("section button")];
    assert.equal(buttons().length, 2);
    buttons()[0].click();
    await until(() => text(app.target).includes("Approved, waits to apply"));
    app.flushSync();
    buttons().at(-1).click();
    await until(() => app.target.querySelectorAll("li.line").length === 4);
    app.flushSync();
    assert.equal(buttons().length, 0);
    // The possible duplicates of the first and of the next page show with their originals.
    assert.ok(app.target.querySelector('a[href="#/item/c-901"]'));
    assert.ok(app.target.querySelector('a[href="#/item/c-902"]'));
    assert.deepEqual(asked, ["handled?limit=50&after=7", "brief/hidden?limit=50&after=c1"]);
  } finally {
    app.destroy();
    globalThis.fetch = saved;
  }
});
