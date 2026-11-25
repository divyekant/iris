// The proposal of the brief (R6-01): the list and the matter are two requests, so the matter can change between them.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const { loadGrowing, loadProposed } = await load("/src/matters.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");
const g5 = fx.matterViews["g-5"];
const as = (id, status = "proposed") => ({ ...g5, matter: { ...g5.matter, id, status } });

// Answer the requests in order: each answer is [path, status, body].
function script(answers) {
  const asked = [];
  globalThis.fetch = async (url) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push(path);
    const next = answers.find((a) => a[0] === path && !a.used) ?? [path, 500, { error: "not in the script" }];
    next.used = true;
    return new Response(JSON.stringify(next[2]), { status: next[1] });
  };
  return asked;
}

const list = (...ids) => ({ proposed: ids.map((id) => as(id).matter), confirmed: [] });

test("a proposal that goes between the requests: the next proposal shows", async () => {
  const asked = script([
    ["matters", 200, list("g-5", "g-6")],
    ["matters/g-5", 404, { error: "not found" }],
    ["matters", 200, list("g-6")],
    ["matters/g-6", 200, as("g-6")],
  ]);
  const p = await loadProposed();
  assert.equal(p.view.matter.id, "g-6");
  assert.equal(p.count, 1);
  assert.deepEqual(asked, ["matters", "matters/g-5", "matters", "matters/g-6"]);
});

test("a proposal that another tab decides between the requests does not show", async () => {
  script([
    ["matters", 200, list("g-5")],
    ["matters/g-5", 200, as("g-5", "confirmed")],
    ["matters", 200, list()],
  ]);
  assert.equal(await loadProposed(), null);
});

test("the retries end: a matter that always goes gives no proposal after 3 lists", async () => {
  const answers = [];
  for (let i = 0; i < 5; i++) answers.push(["matters", 200, list("g-5")], ["matters/g-5", 404, { error: "not found" }]);
  const asked = script(answers);
  assert.equal(await loadProposed(), null);
  assert.equal(asked.length, 6);
});

test("another failure of the matter request is an error, so the brief keeps its last card", async () => {
  script([["matters", 200, list("g-5")], ["matters/g-5", 500, { error: "store" }]]);
  await assert.rejects(loadProposed());
});

test("the brief loads the proposal by itself, and a later load removes it", async () => {
  let proposed = list("g-5");
  globalThis.fetch = async (url) => {
    const path = String(url).replace(/^\/api\//, "");
    const body = path === "matters" ? proposed : path === "matters/g-5" ? as("g-5") : path === "brief" ? fx.brief : path === "status" ? fx.answer("status") : [];
    return new Response(JSON.stringify(body), { status: 200 });
  };
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, watches: [], now: new Date("2026-11-16T09:05:00Z"), poll: 5 });
  try {
    await until(() => /A matter to confirm 1/.test(text(view.target)));
    assert.match(text(view.target), /Iris sees a matter Office lease/);
    proposed = list();
    await until(() => !/Iris sees a matter/.test(text(view.target)));
  } finally {
    view.destroy();
  }
});

test("the brief line of additions lists only the confirmed matters with an addition (D20)", async () => {
  const asked = script([["matters", 200, fx.matters]]);
  const growing = await loadGrowing();
  assert.deepEqual(asked, ["matters"]);
  assert.deepEqual(growing.map((m) => m.id), ["g-4"]);
});

test("a new proposal during the Undo time keeps the card and its hold (issue 345)", async () => {
  let proposed = list("g-5");
  const posts = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    if (init?.method === "POST") return posts.push(path), new Response(JSON.stringify(as("g-5", "declined")), { status: 200 });
    const body = path === "matters" ? proposed : path.startsWith("matters/") ? as(path.slice(8)) : path === "brief" ? fx.brief : path === "status" ? fx.answer("status") : [];
    return new Response(JSON.stringify(body), { status: 200 });
  };
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, watches: [], now: new Date("2026-11-16T09:05:00Z"), poll: 5, hold: 400 });
  const press = (name) => [...view.target.querySelectorAll("button")].find((b) => text(b).trim() === name).click();
  try {
    await until(() => /Iris sees a matter/.test(text(view.target)));
    press("Not a matter");
    await until(() => [...view.target.querySelectorAll("button")].some((b) => text(b).trim() === "Undo"));
    // A newer proposal arrives while the decision waits: the card stays, and nothing is sent.
    proposed = list("g-7", "g-5");
    await new Promise((r) => setTimeout(r, 150));
    assert.deepEqual(posts, [], "the held decision waits for its time");
    press("Undo");
    // After the hold, the brief takes the newest proposal again.
    await until(() => posts.length === 0 && !/Undo/.test(text(view.target)));
  } finally {
    view.destroy();
  }
});
