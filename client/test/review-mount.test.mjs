// The review screen in a DOM: its effects load the target rows, and its buttons work.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const act = (a) => ({ n: 1, revision: 1, verb: "archive", targets: {}, resolved: { messages: [] }, args: {}, why: null, flags: [], hash: "h", decision: { state: "in_review" }, ...a });
const changeset = (actions) => ({ model: 1, id: "cs-9", revision: 1, author: { kind: "agent", name: "Claude Code" }, why: "Tidy the inbox.", created: "2026-11-15T08:00:00Z", submitted: "2026-11-15T08:00:00Z", state: "open", actions });
const text = (el) => el.textContent.replace(/\s+/g, " ");

// Answer each API call from `routes` (status and body), else from the fixtures. Keep a list of the paths.
function serve(routes = {}) {
  const asked = [];
  globalThis.fetch = async (url) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push(path);
    const [status, body] = routes[path] ?? [200, fx.answer(`/${path}`)];
    return new Response(JSON.stringify(body ?? { error: "Not found." }), { status: body === undefined ? 404 : status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

test("a request loads the rows of each message, shows a gone message, and loads the rest with the button", async () => {
  const ids = ["m-50140", "m-1", "m-2", "m-3", "m-4"];
  const asked = serve();
  const view = await mount("/src/views/Request.svelte", { changeset: changeset([act({ resolved: { messages: ids } })]) });
  try {
    const rows = () => view.target.querySelectorAll(".messages li");
    await until(() => rows().length === 3);
    assert.match(text(rows()[0]), /not in your mailbox any more/);
    assert.ok(rows()[1].querySelector("a[href]"), "a message row links to the message");
    assert.equal(asked.filter((p) => p.startsWith("messages?")).length, 1);

    const more = view.target.querySelector("button.more");
    assert.equal(more.textContent.trim(), "Show all 5");
    more.click();
    await until(() => rows().length === 5);
    view.flushSync();
    assert.equal(view.target.querySelector("button.more"), null);
    assert.deepEqual(asked.filter((p) => p.startsWith("messages?")), ["messages?ids=m-50140,m-1,m-2", "messages?ids=m-3,m-4"]);
  } finally {
    view.destroy();
  }
});

test("only a 404 makes an item gone; another failure shows as a problem", async () => {
  const known = fx.items[0].id;
  serve({ "items/c-gone": [404, { error: "Not found." }], "items/c-down": [500, { error: "The store is busy." }] });
  const gone = await mount("/src/views/Request.svelte", { changeset: changeset([act({ verb: "close", targets: { items: [known, "c-gone"] } })]) });
  try {
    await until(() => gone.target.querySelectorAll(".messages li").length === 2);
    assert.match(text(gone.target), /c-gone: not in Iris any more/);
    assert.equal(gone.target.querySelector("[role=alert]"), null);
  } finally {
    gone.destroy();
  }

  const down = await mount("/src/views/Request.svelte", { changeset: changeset([act({ verb: "close", targets: { items: [known, "c-down"] } })]) });
  try {
    const alert = await until(() => down.target.querySelector("[role=alert]"));
    assert.equal(alert.textContent, "The store is busy.");
    assert.doesNotMatch(text(down.target), /not in Iris any more/);
  } finally {
    down.destroy();
  }
});

test("the review shows an open request whose actions all have a decision, with the decision", async () => {
  serve();
  const done = changeset([act({ resolved: { messages: ["m-1"] }, decision: { state: "approved", by: "person", at: "2026-11-15T09:00:00Z" } })]);
  const view = await mount("/src/views/Review.svelte", { changesets: [done] });
  try {
    const page = text(view.target);
    assert.match(page, /Nothing waits for your review\./);
    assert.doesNotMatch(page, /When an agent or Iris asks/);
    assert.match(page, /Claude Code/);
    assert.match(page, /Tidy the inbox\./);
    const details = view.target.querySelector("details");
    assert.ok(details.open, "the decided changes show open when nothing in the request waits");
    assert.match(text(details), /Archive 1 message/i);
    assert.equal(view.target.querySelector(".decide"), null);
  } finally {
    view.destroy();
  }
});
