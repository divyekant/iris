// Server renders of each release 1 view with the fixtures of dev/fixtures.ts.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, render } from "./render.mjs";

const fx = await load("/dev/fixtures.ts");
const item = (id) => fx.items.find((i) => i.id === id);
const text = (html) => html.replace(/<!--[^>]*-->/g, "").replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");

test("the brief shows what you owe, what is owed to you, and conversations, each line with its source mail", async () => {
  const html = await render("/src/views/Brief.svelte", { brief: fx.brief });
  const t = text(html);
  assert.match(t, /You owe 2/);
  assert.match(t, /Review contract section 4/);
  assert.match(t, /Owed to you 1 SDK examples/);
  assert.match(t, /Conversations 1 Dinner on Thursday\?/);
  assert.match(html, /href="#\/item\/c-311"/);
  assert.match(html, /href="#\/mail\/m-55210"/);
  assert.match(t, /Mail Weekly 1 new/);
  assert.match(t, /2 hostile messages in the last 7 days/);
  assert.match(t, /Iris could not understand 1 messages/);
  assert.match(t, /It changed nothing in your mailbox/);
});

test("the brief shows at most 7 lines for each section and links the full list", async () => {
  const many = Array.from({ length: 9 }, (_, n) => ({ ...item("c-311"), id: `c-${n}` }));
  const brief = { ...fx.brief, you_owe: { count: 9, items: many } };
  const html = await render("/src/views/Brief.svelte", { brief });
  assert.equal((html.match(/href="#\/item\/c-\d"/g) ?? []).length, 7);
  assert.match(html, /href="#\/commitments">All 9</);
});

test("the setup flow asks for the account when there is none", async () => {
  const t = text(await render("/src/views/Setup.svelte", { status: { ...fx.status, accounts: [] }, poll: 0 }));
  assert.match(t, /iris account add client\.json/);
  assert.match(t, /gmail\.readonly/);
});

test("the setup flow shows the copy progress and says that the mailbox does not change", async () => {
  const account = { ...fx.status.accounts[0], mirror_ready_at: null, sync: { kind: "first", step: 2 } };
  const status = { accounts: [account], messages: { total: 84212, fetched: 1000, understood: 0, gone: 0 } };
  const t = text(await render("/src/views/Setup.svelte", { status, poll: 0 }));
  assert.match(t, /1,000 of 84,212 messages copied/);
  assert.match(t, /Nothing in your mailbox changes/);
});

test("the setup flow says mirror ready and that nothing changed", async () => {
  const status = { ...fx.status, messages: { total: 84212, fetched: 84212, understood: 100, gone: 0 } };
  const html = await render("/src/views/Setup.svelte", { status, poll: 0 });
  assert.match(text(html), /Mirror ready\. 84,212 messages\. Nothing in your mailbox changed\./);
  assert.match(text(html), /100 of 84,212 messages/);
  assert.match(html, /href="#\/brief"/);
});

test("an item shows its fields and opens each source mail", async () => {
  const t = text(await render("/src/views/Item.svelte", { item: item("v-88") }));
  assert.match(t, /With Sam Ortiz/);
  assert.match(t, /Your turn Yes/);
  const html = await render("/src/views/Item.svelte", { item: item("v-88") });
  for (const m of ["m-55380", "m-55390", "m-55400"]) assert.match(html, new RegExp(`href="#/mail/${m}"`));
});

test("a signal shows its code in large type only before it expires", async () => {
  const signal = item("s-901");
  const before = await render("/src/views/Item.svelte", { item: signal, now: new Date("2026-11-16T09:05:00Z") });
  assert.match(before, /class="code[^"]*"[^>]*>482913</);
  const afterExpiry = await render("/src/views/Item.svelte", { item: signal, now: new Date("2026-11-16T09:20:00Z") });
  assert.doesNotMatch(afterExpiry, /class="code/);
});

test("records show as a table with the fields of the type", async () => {
  const html = await render("/src/views/RecordTable.svelte", { type: "receipt", items: [item("r-1201")] });
  const t = text(html);
  assert.match(t, /vendor amount date order items Source/);
  assert.match(t, /Example Books 24\.50 EUR/);
  assert.match(t, /EB-20931 A book about mail/);
  assert.match(html, /href="#\/mail\/m-53010"/);
});

test("the records view shows one choice for each type with records", async () => {
  const html = await render("/src/views/Records.svelte", {
    counts: { receipt: 1, booking: 1, ticket: 0 },
    type: "receipt",
    page: { items: [item("r-1201")], next: null },
  });
  assert.match(text(html), /receipt 1 booking 1/);
  assert.doesNotMatch(text(html), /ticket/);
  assert.match(text(html), /Example Books/);
});

test("a mail shows as escaped text, with a warning when it is hostile", async () => {
  const message = { ...fx.message("m-55210"), text: '<script>alert(1)</script><img src=x onerror="x()">', hostile: ["dmarc"] };
  const html = await render("/src/views/Mail.svelte", { message });
  assert.doesNotMatch(html, /<script>alert|<img/);
  assert.match(html, /&lt;script>alert\(1\)&lt;\/script>/);
  assert.match(text(html), /This mail looks hostile \(dmarc\)/);
  assert.match(html, /href="\/api\/messages\/m-55210\/raw" download/);
  assert.match(html, /href="#\/item\/c-311"/);
});

test("the measures page shows the open count", async () => {
  const t = text(await render("/src/views/Measures.svelte", { opens: { count: 3, since: "2026-11-16T09:00:00Z" } }));
  assert.match(t, /Source mail opened 3/);
  assert.match(t, /Nothing leaves the machine/);
});

test("each open of a mail adds one to the count, and a bad value starts from zero", async () => {
  const { countOpen, readOpens } = await load("/src/opens.ts");
  const data = new Map();
  const storage = { getItem: (k) => data.get(k) ?? null, setItem: (k, v) => data.set(k, v) };
  assert.deepEqual(readOpens(storage), { count: 0, since: null });
  countOpen(storage, new Date("2026-11-16T09:00:00Z"));
  assert.deepEqual(countOpen(storage), { count: 2, since: "2026-11-16T09:00:00.000Z" });
  data.set("iris.opens", "{bad");
  assert.equal(readOpens(storage).count, 0);
  const full = { getItem: () => null, setItem: () => { throw new Error("quota"); } };
  assert.equal(countOpen(full).count, 1);
});

test("routes in the hash give the view", async () => {
  const { parse } = await load("/src/route.ts");
  assert.deepEqual(parse(""), { view: "home" });
  assert.deepEqual(parse("#/commitments"), { view: "list", form: "commitment" });
  assert.deepEqual(parse("#/mail/m-55210"), { view: "mail", id: "m-55210" });
  assert.deepEqual(parse("#/item/"), { view: "home" });
  assert.deepEqual(parse("#/records"), { view: "records" });
});

test("the fixture API answers the endpoints of the local API", () => {
  assert.equal(fx.answer("items?form=record&type=receipt").items.length, 1);
  assert.equal(fx.answer("items/c-311").id, "c-311");
  assert.equal(fx.answer("messages/m-55210").items[0], "c-311");
  assert.equal(fx.answer("nothing"), undefined);
});
