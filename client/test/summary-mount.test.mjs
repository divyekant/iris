// The summary of a long thread (issue 478): the Summary ready pill of a brief
// line, and the summary box of the item page.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const owe = fx.items.find((i) => i.form === "commitment" && i.direction === "you_owe");
const text = (el) => el.textContent.replace(/\s+/g, " ").trim();

function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push(path);
    const [status, out] = answer(path);
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

test("a line shows Summary ready only when the engine says so, as a link to the item", async () => {
  serve(() => [404, { error: "Not found." }]);
  const cases = [
    [{ ...owe, ready: ["summary"] }, ["Summary ready"]],
    [{ ...owe, ready: ["draft", "summary"] }, ["Draft ready", "Summary ready"]],
    [{ ...owe, ready: ["draft"] }, ["Draft ready"]],
    [{ ...owe, ready: [] }, []],
    [owe, []],
  ];
  for (const [item, want] of cases) {
    const view = await mount("/src/views/Line.svelte", { item });
    try {
      const pills = [...view.target.querySelectorAll(".ready")];
      assert.deepEqual(pills.map(text), want, JSON.stringify(item.ready));
      const summary = view.target.querySelector("a.ready.summary");
      assert.equal(!!summary, want.includes("Summary ready"));
      if (summary) assert.equal(summary.getAttribute("href"), `#/item/${item.id}`);
    } finally {
      view.destroy();
    }
  }
});

test("the item page shows the summary of Iris as text, and nothing on a 404", async () => {
  const summary = { ...fx.summaries["c-311"], text: "Dana asked <b>twice</b>.\nShe waits." };
  const asked = serve((path) => (path === `items/${owe.id}/summary` ? [200, summary] : [404, { error: "Not found." }]));
  const view = await mount("/src/views/Item.svelte", { item: owe });
  try {
    const box = await until(() => view.target.querySelector("section.summary"));
    assert.equal(text(box.querySelector(".by")), "Summary by Iris");
    // A text node, never HTML.
    assert.equal(box.querySelector(".text").textContent, summary.text);
    assert.equal(box.querySelector("b"), null);
    assert.match(text(box.querySelector(".limit")), /^From the mail up to .+\. Read the mail for the exact words\.$/);
    // The box comes first after the title.
    assert.equal(view.target.querySelector("h1").nextElementSibling, box);
    assert.ok(asked.includes(`items/${owe.id}/summary`));
  } finally {
    view.destroy();
  }

  serve(() => [404, { error: "Not found." }]);
  const none = await mount("/src/views/Item.svelte", { item: owe });
  try {
    await new Promise((r) => setTimeout(r, 30));
    assert.equal(none.target.querySelector("section.summary"), null);
  } finally {
    none.destroy();
  }
});
