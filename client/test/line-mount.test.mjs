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

test("a line shows Draft ready only when the engine says a draft is ready", async () => {
  const cases = [
    [{ ...commitment(), ready: ["draft"] }, true],
    [{ ...fx.items.find((i) => i.form === "conversation"), ready: ["draft"] }, true],
    [{ ...commitment(), ready: [] }, false],
    [commitment(), false],
  ];
  for (const [item, shown] of cases) {
    const view = await mount("/src/views/Line.svelte", { item });
    try {
      const pill = view.target.querySelector(".ready");
      assert.equal(!!pill, shown, `${item.form} ready=${JSON.stringify(item.ready)}`);
      if (shown) {
        assert.equal(text(pill).trim(), "Draft ready");
        assert.equal(pill.querySelector("a, button"), null, "the pill is no control: the line opens the item");
      }
    } finally {
      view.destroy();
    }
  }
});

// R5-02: a feed item, a conversation and a signal get their one quiet verb.
test("Skip, Mute and Dismiss wait, then send their verb and call ondone", async () => {
  for (const [id, label, verb, said] of [
    ["f-5001", "Skip", "skip", "Skipped."],
    ["v-88", "Mute", "mute", "Muted."],
    ["s-901", "Dismiss", "dismiss", "Dismissed."],
  ]) {
    const item = fx.items.find((i) => i.id === id);
    const asked = serve(() => [200, item]);
    let done = null;
    const view = await mount("/src/views/Line.svelte", { item, delay: 30, ondone: (x) => (done = x) });
    try {
      assert.ok(!box(view), "no tick box");
      assert.equal(view.target.querySelector("button.away").getAttribute("aria-label"), `${label}: ${item.title}`);
      button(view, label).click();
      await until(() => button(view, "Undo"));
      assert.equal(asked.length, 0, "no request during the hold");
      assert.match(text(view.target), new RegExp(`${said} It leaves your brief\\.`));
      await until(() => done);
      assert.deepEqual(asked, [[`items/${id}/${verb}`, {}]]);
    } finally {
      view.destroy();
    }
  }
  // With no ondone, or on a commitment, no quiet verb shows.
  for (const props of [{ item: fx.items.find((i) => i.id === "v-88") }, { item: commitment(), ondone: () => {} }]) {
    const view = await mount("/src/views/Line.svelte", props);
    try {
      assert.ok(!view.target.querySelector("button.away"));
    } finally {
      view.destroy();
    }
  }
});

// The full sections of the brief (`#/brief/feed` and the others) have the actions of a brief line.
test("a full section of the brief has the quiet verb, and the line leaves after the hold", async () => {
  const feed = fx.items.filter((i) => i.form === "feed" && !["skipped"].includes(i.status));
  const page = { items: feed, next: null };
  const asked = [];
  // The other calls of the app (status, policy) stay pending, so they change nothing.
  globalThis.fetch = (url, init) => {
    const u = String(url).replace(/^\/api\//, "");
    asked.push([u, init?.body ? JSON.parse(init.body) : undefined]);
    if (!u.startsWith("brief/feed") && !u.startsWith("items/")) return new Promise(() => {});
    return Promise.resolve(new Response(JSON.stringify(page), { status: 200, headers: { "content-type": "application/json" } }));
  };
  // The app reads the route from the hash of the page.
  const globals = ["location", "history", "addEventListener", "removeEventListener"];
  for (const name of globals) globalThis[name] = typeof window[name] === "function" ? window[name].bind(window) : window[name];
  window.location.hash = "#/brief/feed";
  const app = await mount("/src/App.svelte", {});
  try {
    await until(() => button(app, "Skip"));
    assert.ok(asked.some(([u]) => u === "brief/feed?limit=50"), JSON.stringify(asked));
  } finally {
    app.destroy();
    window.location.hash = "";
    for (const name of globals) delete globalThis[name];
  }
  const list = await mount("/src/views/List.svelte", { title: "Feed", page, act: true, delay: 30 });
  try {
    const before = list.target.querySelectorAll("li").length;
    button(list, "Skip").click();
    await until(() => list.target.querySelectorAll("li").length === before - 1);
    assert.deepEqual(asked.at(-1), [`items/${feed[0].id}/skip`, {}]);
  } finally {
    list.destroy();
  }
  // A list of a form has no actions.
  const form = await mount("/src/views/List.svelte", { title: "Feed", page });
  try {
    assert.ok(!button(form, "Skip"));
  } finally {
    form.destroy();
  }
});

// The nudge of a late person (card R3-07): the pill and the text of the reminder.
test("a ready draft owed to you shows as a nudge with its text, and only such a line asks", async () => {
  const owed = { ...fx.items.find((i) => i.form === "commitment" && i.direction === "owed_to_you"), ready: ["draft"] };
  const owe = { ...fx.items.find((i) => i.form === "commitment" && i.direction === "you_owe"), ready: ["draft"] };
  const cases = [
    [owed, [200, fx.nudgeDraft], "Nudge ready", fx.nudgeDraft.body, ["threads/t-8700/draft"]],
    [owed, [200, { ...fx.nudgeDraft, nudge: false, to: null }], "Draft ready", null, ["threads/t-8700/draft"]],
    [owed, [404, { error: "Not found." }], "Draft ready", null, ["threads/t-8700/draft"]],
    [owe, [200, fx.nudgeDraft], "Draft ready", null, []],
  ];
  for (const [item, answer, pill, body, want] of cases) {
    const asked = serve(() => answer);
    const view = await mount("/src/views/Line.svelte", { item });
    try {
      await new Promise((r) => setTimeout(r, 20));
      assert.equal(text(view.target.querySelector(".ready")).trim(), pill, `${item.id} ${answer[0]}`);
      const shown = view.target.querySelector(".nudge");
      assert.equal(shown ? shown.textContent : null, body);
      assert.deepEqual(asked.map((a) => a[0]), want);
    } finally {
      view.destroy();
    }
  }
});
