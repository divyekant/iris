// Done, Later and Decline on a commitment in a DOM: the hold with Undo, the
// request, and the answers of the engine (spec 2 section 4).
// The browser is west of UTC. The time zone of the owner comes from the engine.
process.env.TZ = "America/Los_Angeles";
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
  assert.equal(act.first(NOW), "2026-11-17");
  assert.equal(act.nextWeek(NOW), "2026-11-23", "on a Monday, the Monday a week later");
  assert.equal(act.nextWeek(new Date(2026, 10, 22)), "2026-11-23", "on a Sunday, the next day");
  assert.equal(act.first(new Date(2026, 11, 31)), "2027-01-01");
  // In the time zone of the owner (fixed offsets, so the test knows the day
  // without the code under test): the first day is tomorrow there, Next week
  // is the next Monday there, and only today or earlier has started.
  const zones = { "Asia/Tokyo": 9, "Pacific/Kiritimati": 14, "Etc/GMT+12": -12, UTC: 0 };
  const iso = (ms) => new Date(ms).toISOString().slice(0, 10);
  for (let h = 0; h < 24 * 14; h += 0.5) {
    const now = new Date(Date.UTC(2026, 10, 10) + h * 3_600_000);
    for (const [zone, offset] of Object.entries(zones)) {
      const local = now.getTime() + offset * 3_600_000;
      const today = iso(local);
      const at = `${zone} at ${now.toISOString()}`;
      assert.equal(act.today(now, zone), today, at);
      assert.equal(act.first(now, zone), iso(local + 86_400_000), at);
      const week = act.nextWeek(now, zone);
      const ahead = (Date.parse(week) - Date.parse(today)) / 86_400_000;
      assert.ok(new Date(`${week}T12:00:00Z`).getUTCDay() === 1 && ahead >= 1 && ahead <= 7, `${week}: ${at}`);
      assert.ok(act.started(today, now, zone) && !act.started(act.first(now, zone), now, zone), at);
    }
  }
  // No zone, or a zone that the browser does not know: the zone of the browser.
  const evening = new Date(Date.UTC(2026, 10, 17, 5)); // 21:00 on 16 Nov in Los Angeles.
  assert.equal(act.today(evening), "2026-11-16");
  assert.equal(act.today(evening, "Mars/Olympus"), "2026-11-16");
  assert.equal(act.today(evening, "Asia/Tokyo"), "2026-11-17");
  assert.equal(act.said("defer", "2026-11-23"), "Later, on Mon 23 Nov.");
});

test("day() names the right weekday for each date, also before the year 100", () => {
  const names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  let seed = 7;
  const rand = (n) => ((seed = (seed * 1103515245 + 12345) % 2 ** 31), seed % n);
  for (let i = 0; i < 2000; i++) {
    const t = new Date(0);
    t.setUTCFullYear(1 + rand(9999), rand(12), 1 + rand(28));
    const value = t.toISOString().slice(t.getUTCFullYear() > 9999 ? 1 : 0, 10).replace(/^\+0*/, "").padStart(10, "0");
    assert.ok(act.day(value).startsWith(names[t.getUTCDay()]), `${value}: ${act.day(value)}`);
  }
  assert.ok(act.day("0001-01-05").startsWith("Fri"));
});

test("Done waits, then sends close and shows the new item", async () => {
  const asked = serve((path) => [200, { ...commitment("done") }]);
  let changed = null;
  const view = await mount("/src/views/Act.svelte", { item: commitment(), clock: () => NOW, delay: 30, onchange: (i) => (changed = i) });
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
  const view = await mount("/src/views/Act.svelte", { item: commitment(), clock: () => NOW, delay: 200, onchange: () => {} });
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
  const view = await mount("/src/views/Act.svelte", { item: commitment(), clock: () => NOW, delay: 60_000, onchange: () => {} });
  button(view, "Later").click();
  await until(() => button(view, "Next week"));
  button(view, "Next week").click();
  await until(() => button(view, "Undo"));
  view.destroy();
  await until(() => asked.length === 1);
  assert.deepEqual(asked, [["items/c-311/defer", { date: "2026-11-23" }]]);
});

test("the days of Later are in the time zone of the owner, not of the browser", async () => {
  // 17:00 on Sunday in Los Angeles (the browser) is 10:00 on Monday in Tokyo (the owner).
  const sunday = new Date(2026, 10, 22, 17, 0);
  serve(() => [200, commitment()]);
  const view = await mount("/src/views/Act.svelte", { item: commitment(), clock: () => sunday, zone: "Asia/Tokyo", onchange: () => {} });
  try {
    button(view, "Later").click();
    await until(() => button(view, "Tomorrow"));
    assert.match(text(button(view, "Tomorrow")), /Tue 24 Nov/);
    assert.match(text(button(view, "Next week")), /Mon 30 Nov/);
  } finally {
    view.destroy();
  }
});

test("a Sunday evening west of UTC offers Tomorrow and Next week, the same Monday", async () => {
  const sunday = new Date(2026, 10, 22, 17, 0); // 01:00 UTC on Monday.
  serve(() => [200, commitment()]);
  const view = await mount("/src/views/Act.svelte", { item: commitment(), clock: () => sunday, zone: "America/Los_Angeles", onchange: () => {} });
  try {
    button(view, "Later").click();
    await until(() => button(view, "Tomorrow"));
    assert.match(text(button(view, "Tomorrow")), /Mon 23 Nov/);
    assert.equal(button(view, "Next week"), undefined, "Next week is the same day as Tomorrow");
  } finally {
    view.destroy();
  }
});

test("a day that starts during the hold is still sent, also when the page closes (issue #241)", async () => {
  const asked = serve(() => [200, commitment("deferred")]);
  let now = new Date(2026, 10, 16, 23, 59, 58);
  const view = await mount("/src/views/Act.svelte", { item: commitment(), clock: () => now, zone: "America/Los_Angeles", onchange: () => {} });
  try {
    button(view, "Later").click();
    await until(() => button(view, "Tomorrow"));
    button(view, "Tomorrow").click(); // Tue 17 Nov.
    now = new Date(2026, 10, 17, 0, 0, 1);
    window.dispatchEvent(new Event("pagehide"));
    await until(() => asked.length === 1);
    assert.deepEqual(asked, [["items/c-311/defer", { date: "2026-11-17" }]]);
  } finally {
    view.destroy();
  }
});

test("the item view shows Later and the day it comes back", async () => {
  const back = { ...commitment("deferred"), deferred_until: "2026-11-23" };
  serve(() => [200, back]);
  const view = await mount("/src/views/Item.svelte", { item: commitment() });
  try {
    button(view, "Later").click();
    await until(() => view.target.querySelector("input[type=date]"));
    const input = view.target.querySelector("input[type=date]");
    input.value = "2099-01-05";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => button(view, "Undo"));
    window.dispatchEvent(new Event("pagehide"));
    await until(() => text(view.target).includes("Back on"));
    assert.match(text(view.target), /State\s*deferred/);
    assert.match(text(view.target), /Back on\s*Nov 23, 2026/);
  } finally {
    view.destroy();
  }
});

test("a page kept open past midnight offers the new days, and refuses an old one", async () => {
  const asked = serve(() => [200, commitment("deferred")]);
  let now = NOW;
  const view = await mount("/src/views/Act.svelte", { item: commitment(), clock: () => now, delay: 10, onchange: () => {} });
  try {
    now = new Date(2026, 10, 18, 0, 30);
    button(view, "Later").click();
    await until(() => button(view, "Tomorrow"));
    assert.match(text(button(view, "Tomorrow")), new RegExp(act.day(act.first(now))));
    const input = view.target.querySelector("input[type=date]");
    input.value = "2026-11-17";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new Event("change", { bubbles: true }));
    await new Promise((r) => setTimeout(r, 50));
    assert.equal(asked.length, 0, "a day that started is not sent");
  } finally {
    view.destroy();
  }
});

test("a page close in the hold sends the click", async () => {
  const asked = serve(() => [200, commitment("done")]);
  const view = await mount("/src/views/Act.svelte", { item: commitment(), clock: () => NOW, delay: 60_000, onchange: () => {} });
  try {
    button(view, "Done").click();
    await until(() => button(view, "Undo"));
    window.dispatchEvent(new Event("pagehide"));
    await until(() => asked.length === 1);
    assert.deepEqual(asked, [["items/c-311/close", {}]]);
  } finally {
    view.destroy();
  }
});

test("a conflict shows the item as it is now, with no stale actions", async () => {
  const asked = serve((path) => (path.endsWith("/close") ? [409, { error: "c-311 is done: close changes an open or deferred commitment" }] : [200, commitment("done")]));
  let item = commitment();
  const view = await mount("/src/views/Act.svelte", { item, clock: () => NOW, delay: 10, onchange: (i) => (item = i) });
  try {
    button(view, "Done").click();
    await until(() => item.status === "done");
    assert.deepEqual(asked.map(([p]) => p), ["items/c-311/close", "items/c-311"]);
    assert.match(text(view.target), /did not record it: c-311 is done/);
  } finally {
    view.destroy();
  }
});

test("a refusal says why, and no answer says Iris does not know", async () => {
  for (const [answer, want] of [
    [() => [409, { error: "c-311 is done: close changes an open or deferred commitment" }], /did not record it: c-311 is done/],
    [() => { throw new TypeError("network"); }, /does not know if it recorded the change/],
    // An engine that restarted refuses the old token: nothing was written.
    [() => [401, { error: "Sign in again." }], /did not record it/],
  ]) {
    serve(answer);
    const view = await mount("/src/views/Act.svelte", { item: commitment(), clock: () => NOW, delay: 10, onchange: () => {} });
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
    const view = await mount("/src/views/Act.svelte", { item, clock: () => NOW, onchange: () => {} });
    assert.equal(Boolean(button(view, "Done")), has, `${item.id} ${item.status}`);
    view.destroy();
  }
});

// R5-02: the item view of a feed item, a conversation or a signal shows its one quiet verb.
test("the item view sends skip, mute or dismiss after the hold", async () => {
  for (const [id, label, verb] of [
    ["f-5001", "Skip", "skip"],
    ["v-88", "Mute", "mute"],
    ["s-901", "Dismiss", "dismiss"],
  ]) {
    const item = fx.items.find((i) => i.id === id);
    const asked = serve(() => [200, item]);
    let changed = null;
    const view = await mount("/src/views/Act.svelte", { item, clock: () => NOW, delay: 30, onchange: (i) => (changed = i) });
    try {
      assert.ok(!button(view, "Done"), "no commitment verb");
      button(view, label).click();
      await until(() => text(view.target).includes("Undo"));
      await until(() => changed);
      assert.deepEqual(asked, [[`items/${id}/${verb}`, {}]]);
    } finally {
      view.destroy();
    }
  }
  const quiet = act.quiet;
  assert.equal(quiet({ form: "feed", status: "skipped" }), null);
  assert.equal(quiet({ form: "signal", status: "expired" }), null);
  assert.equal(quiet({ form: "conversation", status: "muted" }), null);
  assert.equal(quiet({ form: "record", status: "kept" }), null);
});

test("Reply of a conversation stays while Mute holds, and after Mute fails (R3-10)", async () => {
  const conversation = fx.items.find((i) => i.id === "v-88");
  serve(() => [500, { error: "The store is busy." }]);
  const view = await mount("/src/views/Item.svelte", { item: conversation });
  const reply = () => view.target.querySelector('a[href="#/write/t-9001"]');
  try {
    assert.ok(reply());
    button(view, "Mute").click();
    await until(() => button(view, "Undo"));
    view.flushSync();
    assert.ok(reply(), "during the hold");
    window.dispatchEvent(new Event("pagehide"));
    await until(() => text(view.target).includes("Iris did not record it") || text(view.target).includes("does not know"));
    view.flushSync();
    assert.ok(reply(), "after the failure");
  } finally {
    view.destroy();
  }
});
