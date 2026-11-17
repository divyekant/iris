// Notices by rule (card R5-08): the cards above every view, Show more, and Done.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");
const copy = (x) => JSON.parse(JSON.stringify(x));

function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push([init?.method ?? "GET", path]);
    const [status, out] = await answer(path);
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

const button = (view, name) => [...view.target.querySelectorAll("button")].find((b) => text(b).trim().startsWith(name));
const cards = (view) => [...view.target.querySelectorAll("article")];

test("three notices show, the card opens the mail, and Show more opens the rest", async () => {
  serve(() => [200, {}]);
  const view = await mount("/src/views/Notices.svelte", { first: copy(fx.notices), poll: 0 });
  try {
    assert.equal(cards(view).length, 3);
    const first = cards(view)[0];
    assert.match(text(first), /Mail from the clinic/);
    const link = first.querySelector("a");
    assert.equal(link.getAttribute("href"), "#/mail/m-1");
    assert.equal(text(link), "Harbor Clinic: Your test results are ready");
    // A mail that left the mailbox: words, no link.
    assert.equal(cards(view)[2].querySelector("a"), null);
    assert.match(text(cards(view)[2]), /left your mailbox/);
    button(view, "Show 1 more").click();
    await until(() => cards(view).length === 4);
  } finally {
    view.destroy();
  }
});

test("Done closes the notice; a refusal keeps it with the reason", async () => {
  let status = 500;
  const asked = serve(() => (status === 200 ? [200, { done: "n-4" }] : [500, { error: "database: locked" }]));
  const view = await mount("/src/views/Notices.svelte", { first: copy(fx.notices).slice(0, 1), poll: 0 });
  try {
    button(view, "Done").click();
    await until(() => view.target.querySelector("[role=alert]"));
    assert.equal(cards(view).length, 1);
    status = 200;
    button(view, "Done").click();
    await until(() => cards(view).length === 0);
    assert.deepEqual(asked.at(-1), ["POST", "notices/n-4/done"]);
  } finally {
    view.destroy();
  }
});

test("no notice, no section", async () => {
  serve(() => [200, []]);
  const view = await mount("/src/views/Notices.svelte", { first: [], poll: 0 });
  try {
    assert.equal(view.target.querySelector("section"), null);
  } finally {
    view.destroy();
  }
});

test("the Rules screen has a group for notify rules", async () => {
  const { GROUPS } = await load("/src/rules.ts");
  assert.equal(GROUPS.at(-1).effect, "notify");
  assert.equal(GROUPS.at(-1).title, "Notify");
});

test("with the props of App, the notices load from the engine, and a later poll adds new ones", async () => {
  let list = copy(fx.notices).slice(1, 2);
  const asked = serve((path) => (path === "notices" ? [200, list] : [404, {}]));
  const view = await mount("/src/views/Notices.svelte", {});
  try {
    await until(() => cards(view).length === 1);
    assert.match(text(cards(view)[0]), /Dana Whitfield/);
    view.destroy();
  } catch (e) {
    view.destroy();
    throw e;
  }
  const polled = await mount("/src/views/Notices.svelte", { poll: 20 });
  try {
    await until(() => cards(polled).length === 1);
    // A new notice comes: the next poll shows it.
    list = copy(fx.notices).slice(0, 2);
    await until(() => cards(polled).length === 2);
    assert.ok(asked.filter(([, p]) => p === "notices").length >= 2);
  } finally {
    polled.destroy();
  }
});

test("each fixture notice has its notify rule, and the Rules screen shows the Notify group", async () => {
  const notify = fx.policy.rules.filter((r) => r.effect === "notify");
  for (const n of fx.notices) {
    assert.ok(notify.some((r) => r.id === n.rule && r.name === n.rule_name), n.id);
  }
  assert.ok(fx.policy.next_id > Math.max(...fx.policy.rules.map((r) => r.id)));
  const view = await mount("/src/views/Rules.svelte", { policy: { ...fx.policy, file: { state: "same" } }, every: 0 });
  try {
    assert.match(text(view.target), /Notify.*Mail from the clinic/);
  } finally {
    view.destroy();
  }
});
