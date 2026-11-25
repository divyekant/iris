// The summary of a long thread (issue 478): the Summary ready pill of a brief
// line, and the summary box of the item page.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const act = await load("/src/act.ts");
const format = await load("/src/format.ts");
const owe = fx.items.find((i) => i.form === "commitment" && i.direction === "you_owe");
const text = (el) => el.textContent.replace(/\s+/g, " ").trim();

function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init = {}) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push(init.method === "POST" ? `POST ${path}` : path);
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
  // Iris wrote the summary two days after the newest mail that it read.
  const summary = { ...fx.summaries["c-311"], text: "Dana asked <b>twice</b>.\nShe waits.", based_on: "m-77", created: "2026-11-16T10:00:00Z" };
  const mail = { id: "m-77", thread: "t-1", date: "2026-11-14T09:00:00Z", from: { name: "Dana", address: "dana@acme.example" }, subject: "Terms", labels: [] };
  const asked = serve((path) =>
    path === `items/${owe.id}/summary` ? [200, summary] : path === "messages?ids=m-77" ? [200, { messages: [mail] }] : [404, { error: "Not found." }],
  );
  const view = await mount("/src/views/Item.svelte", { item: owe });
  try {
    const box = await until(() => view.target.querySelector("section.summary"));
    assert.equal(text(box.querySelector(".by")), "Summary by Iris");
    // A text node, never HTML.
    assert.equal(box.querySelector(".text").textContent, summary.text);
    assert.equal(box.querySelector("b"), null);
    // The date of the newest mail of the summary, not the date when Iris wrote it.
    await until(() => text(box.querySelector(".limit")).startsWith("From the mail up to"));
    assert.equal(text(box.querySelector(".limit")), `From the mail up to ${format.when(mail.date)}. Read the mail for the exact words.`);
    assert.notEqual(format.when(mail.date), format.when(summary.created));
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

test("the summary goes when the item stops needing you: Done reads the summary again", async () => {
  let closed = false;
  const summary = { ...fx.summaries["c-311"], based_on: "m-77" };
  const asked = serve((path) => {
    if (path === `items/${owe.id}/close`) return (closed = true), [200, { ...owe, status: "done", updated: "2026-11-16T10:00:00Z" }];
    if (path === `items/${owe.id}/summary`) return closed ? [404, { error: "Not found." }] : [200, summary];
    return [404, { error: "Not found." }];
  });
  const view = await mount("/src/views/Item.svelte", { item: owe });
  try {
    await until(() => view.target.querySelector("section.summary"));
    [...view.target.querySelectorAll("button")].find((b) => text(b).startsWith("Done")).click();
    view.flushSync();
    act.flushHolds();
    await until(() => asked.filter((p) => p === `items/${owe.id}/summary`).length === 2);
    await new Promise((r) => setTimeout(r, 30));
    view.flushSync();
    assert.equal(view.target.querySelector("section.summary"), null);
  } finally {
    view.destroy();
  }
});
