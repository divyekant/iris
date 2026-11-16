// Server renders of each release 1 view with the fixtures of dev/fixtures.ts.
import { test, mock } from "node:test";
import assert from "node:assert/strict";
import { load, render } from "./render.mjs";

const fx = await load("/dev/fixtures.ts");
const item = (id) => fx.items.find((i) => i.id === id);
// The visible text, without the aria-hidden marks of the parties.
const text = (html) => html.replace(/<span[^>]*aria-hidden="true"[^>]*>[^<]*<\/span>/g, "").replace(/<!--[^>]*-->/g, "").replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");
const NOW = new Date("2026-11-16T09:05:00Z");

test("the brief opens with what needs you, in words", async () => {
  const t = text(await render("/src/views/Brief.svelte", { brief: fx.brief, now: NOW }));
  assert.match(t, /You owe two things\. One thing is owed to you\. One conversation waits for you\./);
  const empty = { ...fx.brief, you_owe: { count: 0, items: [] }, owed_to_you: { count: 0, items: [] }, conversations: { count: 0, items: [] } };
  assert.match(text(await render("/src/views/Brief.svelte", { brief: empty, now: NOW })), /Nothing needs you now\./);
});

test("the brief lists what you owe, what is owed to you, and conversations, each line with its source mail", async () => {
  const html = await render("/src/views/Brief.svelte", { brief: fx.brief, now: NOW });
  const t = text(html);
  assert.match(t, /You owe 2 Review contract section 4/);
  assert.match(t, /Owed to you 1 SDK examples/);
  assert.match(t, /Conversations 1 Dinner on Thursday\?/);
  assert.match(t, /Sam Ortiz: Are you free for dinner on Thursday\?/);
  assert.match(t, /Dana Whitfield ?, due tomorrow/);
  assert.match(html, /href="#\/item\/c-311"/);
  assert.match(html, /href="#\/mail\/m-55210" aria-label="Open the source mail"/);
  assert.match(t, /Iris found 2 hostile messages in the last 7 days/);
  assert.match(t, /Iris could not read 1 messages/);
  assert.match(t, /It changed nothing in your mailbox/);
});

test("the brief shows a live login code at the top, and not after it expires", async () => {
  const before = await render("/src/views/Brief.svelte", { brief: fx.brief, now: NOW });
  assert.match(before, /aria-label="Login code"[^>]*>482913</);
  assert.ok(before.indexOf("482913") < before.indexOf('id="you_owe"'));
  const after = await render("/src/views/Brief.svelte", { brief: fx.brief, now: new Date("2026-11-16T09:20:00Z") });
  assert.doesNotMatch(after, /aria-label="Login code"/);
  assert.match(text(after), /Signals 1 Login code for Example Bank/);
});

test("each brief section shows at most 7 lines and links its full list", async () => {
  const many = Array.from({ length: 9 }, (_, n) => ({ ...item("c-311"), id: `c-${n}` }));
  const brief = { ...fx.brief, you_owe: { count: 9, items: many }, strangers: { count: 8, items: many.slice(0, 8) } };
  const html = await render("/src/views/Brief.svelte", { brief, now: NOW });
  assert.match(html, /href="#\/brief\/you_owe">All 9</);
  assert.match(html, /href="#\/brief\/strangers">All 8</);
  const youOwe = html.split('id="you_owe"')[1].split("</section>")[0];
  assert.equal((youOwe.match(/href="#\/item\/c-\d"/g) ?? []).length, 7);
});

test("the brief groups the feed by sender, not by name, and shows at most 7 senders", async () => {
  const feed = (n, sender, name) => ({ ...item("f-5001"), id: `f-${n}`, publisher: { kind: "sender", sender, name } });
  const same = [feed(1, "p-1", "News"), feed(2, "p-2", "News")];
  const t = text(await render("/src/views/Brief.svelte", { brief: { ...fx.brief, feed: { count: 2, items: same } }, now: NOW }));
  assert.match(t, /News 1 News 1/);
  const ten = Array.from({ length: 10 }, (_, n) => feed(n, `p-${n}`, `Sender ${n}`));
  const html = await render("/src/views/Brief.svelte", { brief: { ...fx.brief, feed: { count: 10, items: ten } }, now: NOW });
  assert.equal((html.match(/Sender \d/g) ?? []).length, 7);
  assert.match(html, /href="#\/brief\/feed"[^>]*>All 10</);
});

test("the opening counts the whole conversation section, not only the items that the API gave", async () => {
  const brief = { ...fx.brief, you_owe: { count: 0, items: [] }, owed_to_you: { count: 0, items: [] }, conversations: { count: 230, items: [] } };
  assert.match(text(await render("/src/views/Brief.svelte", { brief, now: NOW })), /230 conversations wait for you\./);
});

test("feed counts show only when the API gave every feed item, and the full feed stays one step away", async () => {
  const feed = Array.from({ length: 3 }, (_, n) => ({ ...item("f-5001"), id: `f-${n}` }));
  const html = await render("/src/views/Brief.svelte", { brief: { ...fx.brief, feed: { count: 250, items: feed } }, now: NOW });
  assert.match(text(html), /Mail Weekly All 250/);
  assert.match(html, /href="#\/brief\/feed"[^>]*>All 250</);
});

test("a full section list keeps the possible duplicates", async () => {
  const page = { items: [item("c-311")], next: null, duplicates: { "c-311": ["c-400"] } };
  const t = text(await render("/src/views/List.svelte", { title: "You owe", page, more: "brief/you_owe?limit=50", cursor: "offset" }));
  assert.match(t, /Maybe the same as c-400/);
});

test("the brief shows a possible duplicate pair as one line", async () => {
  const brief = { ...fx.brief, you_owe: { ...fx.brief.you_owe, duplicates: { "c-311": ["c-400"] } } };
  const html = await render("/src/views/Brief.svelte", { brief, now: NOW });
  assert.match(text(html), /Maybe the same as c-400/);
  assert.match(html, /href="#\/item\/c-400"/);
});

test("during the first sync the brief shows the copy progress and what it covers", async () => {
  const account = { ...fx.status.accounts[0], mirror_ready_at: null };
  const status = { accounts: [account], messages: { total: 84212, fetched: 20000, understood: 500, gone: 0 } };
  const brief = { ...fx.brief, oldest_fetched: "2026-09-01T00:00:00Z" };
  const t = text(await render("/src/views/Brief.svelte", { brief, status, now: NOW }));
  assert.match(t, /Iris still copies your mailbox: 20,000 of 84,212 messages\. This brief covers the mail since/);
});

test("the home view is the brief as soon as the first sync has understood mail", async () => {
  const { home } = await load("/src/format.ts");
  const account = { ...fx.status.accounts[0], mirror_ready_at: null };
  const msgs = (understood) => ({ total: 100, fetched: 50, understood, gone: 0 });
  assert.equal(home({ accounts: [], messages: msgs(0) }), "setup");
  assert.equal(home({ accounts: [account], messages: msgs(0) }), "setup");
  assert.equal(home({ accounts: [account], messages: msgs(3) }), "brief");
  assert.equal(home(fx.status), "brief");
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
  assert.match(html, /href="#\/brief">Open the brief</);
});

test("an item shows the quote of its mail first, its fields, and opens each source mail", async () => {
  const html = await render("/src/views/Item.svelte", { item: item("v-88"), now: NOW });
  const t = text(html);
  assert.ok(t.indexOf("Dinner on Thursday? Sam Ortiz, in the mail that made this item") < t.indexOf("With Sam Ortiz"));
  assert.match(t, /Next step Your reply/);
  for (const m of ["m-55380", "m-55390", "m-55400"]) assert.match(html, new RegExp(`href="#/mail/${m}"`));
});

test("a signal page shows its code in large type only before it expires", async () => {
  const before = await render("/src/views/Item.svelte", { item: item("s-901"), now: NOW });
  assert.match(before, /aria-label="Login code"[^>]*>482913</);
  const afterExpiry = await render("/src/views/Item.svelte", { item: item("s-901"), now: new Date("2026-11-16T09:20:00Z") });
  assert.doesNotMatch(afterExpiry, /aria-label="Login code"/);
});

test("the clock ticks until the expiry and one time at the exact moment", async () => {
  const { watch } = await load("/src/clock.ts");
  mock.timers.enable({ apis: ["setTimeout", "Date"], now: Date.parse("2026-11-16T09:14:15Z") });
  try {
    const ticks = [];
    const stop = watch("2026-11-16T09:15:00Z", (d) => ticks.push(d.toISOString()), 30_000);
    mock.timers.tick(30_000);
    mock.timers.tick(15_000);
    mock.timers.tick(60_000);
    assert.deepEqual(ticks, ["2026-11-16T09:14:45.000Z", "2026-11-16T09:15:00.000Z"]);
    stop();
  } finally {
    mock.timers.reset();
  }
});

test("poll loads again until the value says done, and a failed load tries again", async () => {
  const { poll } = await load("/src/clock.ts");
  mock.timers.enable({ apis: ["setTimeout"] });
  const flush = () => new Promise((r) => setImmediate(r));
  try {
    const answers = [Promise.reject(new Error("restart")), Promise.resolve(1), Promise.resolve(2), Promise.resolve(3)];
    const seen = [];
    poll(() => answers.shift(), (v) => seen.push(v), 5000, (v) => v === 2);
    for (let i = 0; i < 5; i++) {
      mock.timers.tick(5000);
      await flush();
    }
    assert.deepEqual(seen, [1, 2]);
    assert.equal(answers.length, 1);
    const stopped = [];
    const stop = poll(() => Promise.resolve(9), (v) => stopped.push(v), 5000);
    stop();
    mock.timers.tick(20000);
    await flush();
    assert.deepEqual(stopped, []);
  } finally {
    mock.timers.reset();
  }
});

test("records show as a table with the fields of the type", async () => {
  const html = await render("/src/views/RecordTable.svelte", { type: "receipt", items: [item("r-1201")] });
  const t = text(html);
  assert.match(t, /vendor amount date order items Source/);
  assert.match(t, /Example Books 24\.50 EUR/);
  assert.match(t, /EB-20931 A book about mail/);
  assert.match(html, /href="#\/mail\/m-53010"/);
});

test("the records view shows one choice for each type, and more records on request", async () => {
  const props = { counts: { receipt: 1, booking: 1, ticket: 0 }, type: "receipt", page: { items: [item("r-1201")], next: null } };
  const html = await render("/src/views/Records.svelte", props);
  assert.match(text(html), /receipt 1 booking 1/);
  assert.doesNotMatch(text(html), /ticket/);
  assert.match(text(html), /Example Books/);
  assert.doesNotMatch(html, /Show more/);
  const more = await render("/src/views/Records.svelte", { ...props, page: { ...props.page, next: "r-1000" } });
  assert.match(more, />Show more</);
});

test("a list pages through its API path with Show more", async () => {
  const page = { items: [item("c-311")], next: 50 };
  const html = await render("/src/views/List.svelte", { title: "You owe", page, more: "brief/you_owe?limit=50" });
  assert.match(html, />Show more</);
  const last = await render("/src/views/List.svelte", { title: "You owe", page: { ...page, next: null }, more: "brief/you_owe?limit=50" });
  assert.doesNotMatch(last, /Show more/);
});

test("the brief shows the triage proposal and says that nothing is applied", async () => {
  const t = text(await render("/src/views/Brief.svelte", { brief: fx.brief, now: NOW }));
  assert.match(t, /Iris would make 447 changes to your mailbox: archive 412, label 35\. Nothing is applied\./);
  const none = text(await render("/src/views/Brief.svelte", { brief: { ...fx.brief, proposal: null }, now: NOW }));
  assert.doesNotMatch(none, /Triage proposal/);
});

test("a stranger conversation shows once, under strangers", async () => {
  // The API gives it only under strangers.
  const brief = { ...fx.brief, conversations: { count: 0, items: [] }, strangers: { count: 1, items: [item("v-88")] } };
  const html = await render("/src/views/Brief.svelte", { brief, now: NOW, poll: 0 });
  assert.equal((html.match(/href="#\/item\/v-88"/g) ?? []).length, 1);
  assert.match(html, /id="strangers"/);
  assert.doesNotMatch(html, /id="conversations"/);
  assert.doesNotMatch(text(html), /conversation waits/);
});

test("copy says so when the browser refuses the clipboard", async () => {
  const { copyText } = await load("/src/format.ts");
  const written = [];
  assert.equal(await copyText("482913", { writeText: async (t) => written.push(t) }), true);
  assert.deepEqual(written, ["482913"]);
  assert.equal(await copyText("482913", { writeText: () => Promise.reject(new DOMException("denied", "NotAllowedError")) }), false);
  assert.equal(await copyText("482913", undefined), false);
});

test("the setup flow opens the first brief while the copy continues", async () => {
  const account = { ...fx.status.accounts[0], mirror_ready_at: null };
  const status = { accounts: [account], messages: { total: 84212, fetched: 9000, understood: 40, gone: 0 } };
  const html = await render("/src/views/Setup.svelte", { status, poll: 0 });
  assert.match(text(html), /Your first brief is ready/);
  assert.match(html, /href="#\/brief"/);
  const early = await render("/src/views/Setup.svelte", { status: { ...status, messages: { ...status.messages, understood: 0 } }, poll: 0 });
  assert.doesNotMatch(early, /href="#\/brief"/);
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

test("a mail from a hostile sender shows the warning with no mark", async () => {
  const message = { ...fx.message("m-55210"), hostile: [], sender_kind: "hostile" };
  assert.match(text(await render("/src/views/Mail.svelte", { message })), /This mail looks hostile \(hostile sender\)/);
  const calm = { ...fx.message("m-55210"), hostile: [], sender_kind: "person" };
  assert.doesNotMatch(await render("/src/views/Mail.svelte", { message: calm }), /looks hostile/);
});

test("a gone mail says how long its content stays, and a removed mail offers no download", async () => {
  const kept = text(await render("/src/views/Mail.svelte", { message: { ...fx.message("m-55210"), gone: true } }));
  assert.match(kept, /Iris keeps its content for 30 days/);
  const html = await render("/src/views/Mail.svelte", { message: { ...fx.message("m-55210"), gone: true, text: null } });
  assert.match(text(html), /Iris removed its content/);
  assert.doesNotMatch(html, /Download the original|<pre/);
});

test("the next day starts at local midnight", async () => {
  const { nextDay } = await load("/src/format.ts");
  const at = new Date(2026, 10, 16, 23, 59);
  assert.equal(nextDay(at), new Date(2026, 10, 17).toISOString());
  assert.equal(new Date(nextDay(at)).getHours(), 0);
});

test("the fixture server gives the original of a mail", () => {
  const source = fx.raw("m-55210");
  assert.match(source, /^From: Example Sender <sender@example\.com>\r\n/);
  assert.match(source, /\r\nSubject: Review contract section 4\r\n/);
  assert.equal(fx.raw("m-0"), undefined);
});

test("the measures page shows the open count", async () => {
  const t = text(await render("/src/views/Measures.svelte", { opens: { count: 3, since: "2026-11-16T09:00:00Z" } }));
  assert.match(t, /3 Source mail opened/);
  assert.match(t, /Nothing leaves the machine/);
});

test("each open of a mail adds one to the count, and refused storage does not stop the page", async () => {
  const { countOpen, readOpens } = await load("/src/opens.ts");
  const data = new Map();
  const storage = () => ({ getItem: (k) => data.get(k) ?? null, setItem: (k, v) => data.set(k, v) });
  assert.deepEqual(readOpens(storage), { count: 0, since: null });
  countOpen(storage, new Date("2026-11-16T09:00:00Z"));
  assert.deepEqual(countOpen(storage), { count: 2, since: "2026-11-16T09:00:00.000Z" });
  data.set("iris.opens", "{bad");
  assert.equal(readOpens(storage).count, 0);
  const full = () => ({ getItem: () => null, setItem: () => { throw new Error("quota"); } });
  assert.equal(countOpen(full).count, 1);
  // A browser that blocks storage throws in the getter of localStorage itself.
  const denied = () => { throw new DOMException("denied", "SecurityError"); };
  assert.deepEqual(readOpens(denied), { count: 0, since: null });
  assert.equal(countOpen(denied).count, 1);
});

test("due dates read as words, with a tone for soon and late", async () => {
  const { due } = await load("/src/format.ts");
  const at = (date) => ({ ...item("c-311"), due: { date } });
  const now = new Date(2026, 10, 16, 12);
  assert.deepEqual(due(at("2026-11-17"), now), { text: "due tomorrow", tone: "soon" });
  assert.deepEqual(due(at("2026-11-26"), now), { text: "due in 10 days", tone: "" });
  assert.deepEqual(due(at("2026-11-14"), now), { text: "was due 2 days ago", tone: "late" });
});

test("the API client asks for the sign-in address on 401 and gives the API error text", async () => {
  const { get, SIGN_IN } = await load("/src/api.ts");
  const answer = (status, body) => async () => new Response(JSON.stringify(body), { status });
  const saved = globalThis.fetch;
  try {
    globalThis.fetch = answer(401, { error: "no session" });
    await assert.rejects(get("brief"), { message: SIGN_IN });
    globalThis.fetch = answer(400, { error: "unknown status" });
    await assert.rejects(get("items?status=x"), { message: "unknown status" });
    globalThis.fetch = answer(200, { ok: 1 });
    assert.deepEqual(await get("status"), { ok: 1 });
  } finally {
    globalThis.fetch = saved;
  }
});

test("routes in the hash give the view", async () => {
  const { parse } = await load("/src/route.ts");
  assert.deepEqual(parse(""), { view: "home" });
  assert.deepEqual(parse("#/commitments"), { view: "list", form: "commitment" });
  assert.deepEqual(parse("#/mail/m-55210"), { view: "mail", id: "m-55210" });
  assert.deepEqual(parse("#/item/"), { view: "home" });
  assert.deepEqual(parse("#/records/receipt"), { view: "records", type: "receipt" });
  assert.deepEqual(parse("#/brief/strangers"), { view: "section", key: "strangers" });
  assert.deepEqual(parse("#/brief/nothing"), { view: "brief" });
  assert.deepEqual(parse("#/mail/%"), { view: "home" });
});

test("the fixture API answers the endpoints of the local API", () => {
  assert.equal(fx.answer("items?form=record&type=receipt").items.length, 1);
  assert.equal(fx.answer("items/c-311").id, "c-311");
  assert.equal(fx.answer("messages/m-55210").items[0], "c-311");
  assert.equal(fx.answer("brief/you_owe?limit=50").items.length, 2);
  assert.equal(fx.answer("nothing"), undefined);
});
