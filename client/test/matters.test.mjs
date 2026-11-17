// Matters (R6-01): the routes, the list and one matter, rendered on the server with the fixtures.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, render } from "./render.mjs";

const fx = await load("/dev/fixtures.ts");
const { parse, href, NAV, MATTER_ID } = await load("/src/route.ts");
const { matterCounts } = await load("/src/format.ts");
const text = (html) => html.replace(/<!--[^>]*-->/g, "").replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");

test("the routes of matters", () => {
  assert.deepEqual(parse("#/matters"), { view: "matters" });
  assert.deepEqual(parse("#/matters/"), { view: "matters" });
  assert.deepEqual(parse("#/matters/g-4"), { view: "matter", id: "g-4" });
  assert.equal(href.matter("g-4"), "#/matters/g-4");
  assert.ok(NAV.some(([link, label]) => link === "#/matters" && label === "Matters"));
  for (const bad of ["#/matters/g-4/x", "#/matters/g-04", "#/matters/4", "#/matters/g-", "#/matters/j-4", "#/matters/g-1%2F2", "#/matters//g-1"]) {
    assert.deepEqual(parse(bad), { view: "home" }, bad);
  }
  // The job routes keep their rules.
  assert.deepEqual(parse("#/jobs/j-12"), { view: "job", id: "j-12" });
  assert.deepEqual(parse("#/jobs/g-12"), { view: "home" });
});

test("generated hashes: #/matters/<id> opens a matter only for one canonical matter ID and no other part", () => {
  let seed = 7;
  const rand = (n) => ((seed = (seed * 1103515245 + 12345) % 2147483648), seed % n);
  const pieces = ["#/matters", "/", "g-", "j-", "0", "1", "9", "x", "%", "%2F", "-", "matters", "#"];
  let opened = 0;
  for (let i = 0; i < 5000; i++) {
    const start = ["#/matters/g-", "#/matters/", "#matters/g-", ""][rand(4)];
    const hash = start + Array.from({ length: rand(6) }, () => pieces[rand(pieces.length)]).join("");
    const route = parse(hash);
    if (route.view === "matter") {
      assert.equal(hash.replace(/^#\/?/, ""), `matters/${route.id}`, hash);
      assert.match(route.id, MATTER_ID, hash);
      opened++;
    }
    if (route.view === "matters") assert.match(hash.replace(/^#\/?/, ""), /^matters\/?$/, hash);
  }
  assert.ok(opened > 0, "some generated hashes open a matter");
});

test("the counts of a matter, by form", () => {
  assert.equal(matterCounts({ record: 2, conversation: 1, commitment: 1 }), "Two records, one conversation, one commitment");
  assert.equal(matterCounts({ record: 0, conversation: 12, commitment: 0 }), "12 conversations");
  assert.equal(matterCounts({ record: 0, conversation: 0, commitment: 0 }), "No items now");
});

test("the list shows the proposals first, then the matters that you kept, each with a link", async () => {
  const html = await render("/src/views/Matters.svelte", { list: fx.matters });
  const t = text(html);
  assert.ok(t.indexOf("Iris proposes") < t.indexOf("Office lease"));
  assert.ok(t.indexOf("Office lease") < t.indexOf("Yours"));
  assert.ok(t.indexOf("Yours") < t.indexOf("Tokyo trip"));
  assert.match(html, /href="#\/matters\/g-4"/);
  assert.match(html, /href="#\/matters\/g-5"/);
  assert.match(t, /One conversation, one commitment|One record, one commitment/);
  const empty = text(await render("/src/views/Matters.svelte", { list: { proposed: [], confirmed: [] } }));
  assert.match(empty, /No matters yet/);
  assert.doesNotMatch(empty, /Iris proposes\b(?! a matter)|Yours/);
});

test("a confirmed matter shows its commitments, records and conversations", async () => {
  const view = fx.matterViews["g-4"];
  const t = text(await render("/src/views/Matter.svelte", { view, now: new Date("2026-11-17T09:00:00Z") }));
  assert.match(t, /Tokyo trip/);
  assert.match(t, /Three items\. Kept together /);
  assert.match(t, /Kept together /);
  const order = ["You owe", "Records", "Conversations"].map((h) => t.indexOf(h));
  assert.ok(order.every((at, i) => at >= 0 && (i === 0 || at > order[i - 1])), String(order));
  assert.doesNotMatch(t, /Owed to you/);
  for (const list of Object.values(view.items)) for (const item of list) assert.ok(t.includes(item.title), item.title);
  assert.doesNotMatch(t, /Iris proposes this matter/);
});

test("a proposal shows on its card with each member and the two decisions; a declined matter says so", async () => {
  const view = fx.matterViews["g-5"];
  const t = text(await render("/src/views/Matter.svelte", { view }));
  assert.match(t, /Iris sees a matter/);
  assert.match(t, /Keep together Not a matter/);
  for (const list of Object.values(view.items)) for (const item of list) assert.ok(t.includes(item.title), item.title);
  assert.doesNotMatch(t, /Kept together|Owed to you/);
  const declined = { ...view, matter: { ...view.matter, status: "declined" } };
  const d = text(await render("/src/views/Matter.svelte", { view: declined }));
  assert.match(d, /You said that this is not a matter/);
  assert.match(d, /Owed to you/);
  assert.doesNotMatch(d, /Keep together/);
  const none = { matter: { ...view.matter, status: "confirmed", counts: { record: 0, conversation: 0, commitment: 0 } }, items: { record: [], conversation: [], commitment: [] } };
  assert.match(text(await render("/src/views/Matter.svelte", { view: none })), /No items now/);
});

test("the brief shows the newest proposal on a card, with the count of all", async () => {
  const proposed = { view: fx.matterViews["g-5"], count: 2 };
  const t = text(await render("/src/views/Brief.svelte", { brief: fx.brief, proposed, watches: [], poll: 0 }));
  assert.match(t, /Matters to confirm 2/);
  assert.match(t, /Iris sees a matter Office lease/);
  assert.match(t, /All 2 that Iris proposes/);
  const one = text(await render("/src/views/Brief.svelte", { brief: fx.brief, proposed: { ...proposed, count: 1 }, watches: [], poll: 0 }));
  assert.match(one, /A matter to confirm 1/);
  assert.doesNotMatch(one, /that Iris proposes/);
  const none = text(await render("/src/views/Brief.svelte", { brief: fx.brief, proposed: null, watches: [], poll: 0 }));
  assert.doesNotMatch(none, /matter to confirm|Iris sees a matter/i);
});
