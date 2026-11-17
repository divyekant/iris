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
  const empty = { ...fx.brief, you_owe: { count: 0, items: [] }, owed_to_you: { count: 0, items: [] }, conversations: { count: 0, items: [] }, strangers: { count: 0, items: [] }, review: { changesets: 0, actions: 0 }, gate: undefined };
  assert.match(text(await render("/src/views/Brief.svelte", { brief: empty, now: NOW })), /Nothing needs you now\./);
  // Only stranger items: the opening must not say that nothing needs you.
  const strangers = { ...empty, strangers: { count: 2, items: [] } };
  const t2 = text(await render("/src/views/Brief.svelte", { brief: strangers, now: NOW }));
  assert.doesNotMatch(t2, /Nothing needs you now/);
  assert.match(t2, /Two items are from senders you do not know\./);
  assert.match(text(await render("/src/views/Brief.svelte", { brief: { ...empty, strangers: { count: 1, items: [] } }, now: NOW })), /One item is from a sender you do not know\./);
  // With the stranger gate, the opening counts the senders that wait for an answer.
  const gate = (n) => ({ count: n, senders: fx.gate.senders.slice(0, n), next: null });
  assert.match(text(await render("/src/views/Brief.svelte", { brief: { ...empty, strangers: { count: 4, items: [] }, gate: gate(3) }, now: NOW })), /Three senders that you do not know wait for an answer\./);
  assert.match(text(await render("/src/views/Brief.svelte", { brief: { ...empty, strangers: { count: 1, items: [] }, gate: gate(1) }, now: NOW })), /One sender that you do not know waits for an answer\./);
  assert.match(text(await render("/src/views/Brief.svelte", { brief: { ...empty, gate: gate(0) }, now: NOW })), /Nothing needs you now\./);
});

test("the stranger gate shows each sender once, with what they ask and the answers", async () => {
  const html = await render("/src/views/Brief.svelte", { brief: fx.brief, now: NOW, poll: 0 });
  const gate = html.split('id="strangers"')[1].split("</section>")[0];
  const t = text(gate);
  assert.match(t, /From senders you do not know 3/);
  assert.match(t, /Maria Ruiz maria@brightpath\.example Intro from Sam Ortiz Review the pitch deck before Friday/);
  assert.match(t, /billing@vendorhub\.example Confirm the address for the invoice/);
  assert.equal((gate.match(/>Accept</g) ?? []).length, 3);
  assert.equal((gate.match(/>Ignore</g) ?? []).length, 3);
  assert.match(gate, /aria-label="Accept Maria Ruiz"/);
  assert.match(gate, /href="#\/brief\/declined"[^>]*>Ignored 1</);
  assert.match(gate, /href="#\/rules"[^>]*>Let a rule answer</);
  // An ask from a stranger is not under You owe before you accept.
  const owe = html.split('id="you_owe"')[1]?.split("</section>")[0] ?? "";
  assert.doesNotMatch(owe, /c-401|c-403|c-404/);
  // More senders than the brief shows: a link to all of them.
  const more = await render("/src/views/Brief.svelte", { brief: { ...fx.brief, gate: { ...fx.gate, count: 25, next: "k" } }, now: NOW, poll: 0 });
  assert.match(more, /href="#\/brief\/gate"[^>]*>All 25</);
  // No sender waits, but some were ignored: the section stays for the link.
  const quiet = text(await render("/src/views/Brief.svelte", { brief: { ...fx.brief, gate: { count: 0, senders: [], next: null } }, now: NOW, poll: 0 }));
  assert.match(quiet, /No sender that you do not know waits for an answer\. Ignored 1/);
});

test("the brief lists what you owe, what is owed to you, and conversations, each line with its source mail", async () => {
  const html = await render("/src/views/Brief.svelte", { brief: fx.brief, now: NOW });
  const t = text(html);
  assert.match(t, /You owe 1 Pay invoice/);
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

test("what you owe in 2 days or less, or late, leads the brief, and You owe lists the rest", async () => {
  const t = text(await render("/src/views/Brief.svelte", { brief: fx.brief, now: NOW }));
  // The due section comes before the review and before You owe.
  assert.match(t, /Due soon 1 Review contract section 4 Dana Whitfield ?, due tomorrow Draft ready Waits for your review/);
  assert.match(t, /You owe 1 Pay invoice .* All 2 Owed to you/);
  // A late thing is due soon too; a thing due in 3 days is not.
  const at = (date) => ({ ...item("c-311"), due: { date } });
  const late = { ...fx.brief, you_owe: { count: 2, items: [at("2026-11-10"), { ...at("2026-11-19"), id: "c-999" }] } };
  const t2 = text(await render("/src/views/Brief.svelte", { brief: late, now: NOW }));
  assert.match(t2, /Due soon 1 Review contract section 4 Dana Whitfield ?, was due 6 days ago/);
  assert.match(t2, /You owe 1 Review contract section 4 Dana Whitfield ?, due in 3 days All 2/);
  // Nothing due soon: no section, and You owe has every thing.
  const none = text(await render("/src/views/Brief.svelte", { brief: { ...fx.brief, you_owe: { count: 1, items: [late.you_owe.items[1]] } }, now: NOW }));
  assert.doesNotMatch(none, /Due soon/);
  assert.match(none, /You owe 1 /);
});

test("the brief shows a live login code at the top, and not after it expires", async () => {
  const before = await render("/src/views/Brief.svelte", { brief: fx.brief, now: NOW });
  assert.match(before, /aria-label="Login code"[^>]*>482913</);
  assert.ok(before.indexOf("482913") < before.indexOf('id="you_owe"'));
  const after = await render("/src/views/Brief.svelte", { brief: fx.brief, now: new Date("2026-11-16T09:20:00Z") });
  assert.doesNotMatch(after, /aria-label="Login code"/);
  // Spec 1 section 8: Signals holds a signal only while its expiry is later than now.
  assert.doesNotMatch(text(after), /Login code for Example Bank/);
  assert.doesNotMatch(after, /id="signals"/);
});

test("the Signals heading keeps the exact count of the API when a live code shows at the top", async () => {
  const code = item("s-901");
  const rest = Array.from({ length: 8 }, (_, n) => ({ ...code, id: `s-${n}`, code: undefined, title: `Signal ${n}` }));
  const brief = { ...fx.brief, signals: { count: 9, items: [code, ...rest] } };
  const t = text(await render("/src/views/Brief.svelte", { brief, now: NOW }));
  assert.match(t, /Signals 9/);
  assert.match(t, /All 9/);
  // With only the live code, the code shows at the top, and the Signals section has its heading and link but no empty list.
  const only = await render("/src/views/Brief.svelte", { brief: { ...fx.brief, signals: { count: 1, items: [code] } }, now: NOW });
  assert.match(only, /<h2 id="signals"[^>]*><span>Signals<\/span> <span[^>]*>1<\/span><\/h2> <!--\[-1--><!--\]--> <a class="all/);
});

test("the dev proxy sends the Host of the engine, and changes only the dev page's own Origin", async () => {
  const { engineProxy } = await load("/dev/proxy.ts");
  const target = "http://127.0.0.1:4800";
  const options = engineProxy(target);
  assert.equal(options.target, target);
  assert.equal(options.changeOrigin, true);
  let send;
  options.configure({ on: (event, fn) => event === "proxyReq" && (send = fn) });
  const forward = (headers) => {
    const out = {};
    send({ setHeader: (k, v) => (out[k] = v) }, { headers });
    return out;
  };
  // The sign-in address and API calls from the dev page reach the engine as same-origin requests.
  assert.deepEqual(forward({ host: "localhost:5173", origin: "http://localhost:5173" }), { origin: target });
  // Another site keeps its Origin, so the engine refuses it.
  assert.deepEqual(forward({ host: "localhost:5173", origin: "https://example.com" }), {});
  assert.deepEqual(forward({ host: "localhost:5173" }), {});
});

test("each brief section shows at most 7 lines and links its full list", async () => {
  const many = Array.from({ length: 9 }, (_, n) => ({ ...item("c-311"), id: `c-${n}`, due: { date: "2026-12-20" } }));
  // An API from before the stranger gate: strangers is a section of items.
  const brief = { ...fx.brief, you_owe: { count: 9, items: many }, strangers: { count: 8, items: many.slice(0, 8) }, gate: undefined };
  const html = await render("/src/views/Brief.svelte", { brief, now: NOW });
  assert.match(html, /href="#\/brief\/you_owe">All 9</);
  assert.match(html, /href="#\/brief\/strangers">All 8</);
  const youOwe = html.split('id="you_owe"')[1].split("</section>")[0];
  assert.equal((youOwe.match(/href="#\/item\/c-\d"/g) ?? []).length, 7);
  // Due soon has the same limit, and its link opens all that you owe.
  const due = Array.from({ length: 9 }, (_, n) => ({ ...item("c-311"), id: `c-${n}` }));
  const html2 = await render("/src/views/Brief.svelte", { brief: { ...fx.brief, you_owe: { count: 9, items: due } }, now: NOW });
  const soon = html2.split('id="soon"')[1].split("</section>")[0];
  assert.equal((soon.match(/href="#\/item\/c-\d"/g) ?? []).length, 7);
  assert.match(soon, /href="#\/brief\/you_owe">All 9 that you owe</);
  assert.doesNotMatch(html2, /id="you_owe"/);
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
  const t = text(await render("/src/views/List.svelte", { title: "You owe", page, more: "brief/you_owe?limit=50", cursor: "after" }));
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
  // The Brief link of the header: the brief while the first sync runs, and setup only when there is no account.
  assert.equal(home({ accounts: [], messages: msgs(0) }, "brief"), "setup");
  assert.equal(home({ accounts: [account], messages: msgs(0) }, "brief"), "brief");
});

test("the setup flow asks for the account when there is none", async () => {
  const t = text(await render("/src/views/Setup.svelte", { status: { ...fx.status, accounts: [] }, poll: 0 }));
  assert.match(t, /iris account add client\.json/);
  assert.match(t, /gmail\.readonly/);
  // A client in "Testing" gets refresh tokens that end after 7 days (spec 1 section 4.9).
  assert.match(t, /On the Audience page, click "Publish app"/);
  assert.ok(t.indexOf("Publish app") < t.indexOf("iris account add"), "publish before sign-in");
});

test("the setup flow shows the copy progress and says that the mailbox does not change", async () => {
  const account = { ...fx.status.accounts[0], mirror_ready_at: null, sync: { kind: "first", step: 2 } };
  const status = { accounts: [account], messages: { total: 84212, fetched: 1000, understood: 0, gone: 0 } };
  const t = text(await render("/src/views/Setup.svelte", { status, poll: 0 }));
  assert.match(t, /1,000 of 84,212 messages copied/);
  assert.match(t, /Nothing in your mailbox changes/);
});

test("a conversation item opens the reply form of its thread, unless the conversation is gone (R3-10)", async () => {
  const html = await render("/src/views/Item.svelte", { item: item("v-88"), now: NOW });
  assert.match(html, /<a class="reply[^"]*" href="#\/write\/t-9001">Reply</);
  // Reply leads the row of the conversation verbs.
  assert.match(text(html), /Reply Mute/);
  // A muted conversation has no row of verbs: Reply still shows.
  assert.match(await render("/src/views/Item.svelte", { item: { ...item("v-88"), status: "muted" }, now: NOW }), /href="#\/write\/t-9001">Reply</);
  const gone = await render("/src/views/Item.svelte", { item: { ...item("v-88"), status: "gone" }, now: NOW });
  assert.doesNotMatch(gone, /href="#\/write\//);
  // Only a conversation has the reply: a commitment of the same thread does not.
  assert.doesNotMatch(await render("/src/views/Item.svelte", { item: item("c-311"), now: NOW }), /href="#\/write\//);
});

test("the setup flow says mirror ready and that nothing changed", async () => {
  const status = { ...fx.status, accounts: [fx.status.accounts[0]], messages: { total: 84212, fetched: 84212, understood: 100, gone: 0 } };
  const html = await render("/src/views/Setup.svelte", { status, poll: 0 });
  assert.match(text(html), /Mirror ready\. 84,212 messages\. Nothing in your mailbox changed\./);
  assert.match(text(html), /100 of 84,212 messages/);
  assert.match(html, /href="#\/brief">Open the brief</);
});

test("an item shows the quote of its mail first, its fields, and opens each source mail", async () => {
  const html = await render("/src/views/Item.svelte", { item: item("v-88"), now: NOW });
  const t = text(html);
  // The client does not know who wrote the origin mail, so the caption names no sender.
  assert.ok(t.indexOf("Dinner on Thursday? From the mail that made this item") < t.indexOf("With Sam Ortiz"));
  assert.doesNotMatch(t, /Sam Ortiz, in the mail/);
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

test("the brief puts the stored triage proposal under the review, and has no separate proposal section", async () => {
  const t = text(await render("/src/views/Brief.svelte", { brief: fx.brief, now: NOW }));
  assert.match(t, /Waits for your review 4 Triage proposes to archive 412 messages, label 35 messages\. Open the review/);
  assert.doesNotMatch(t, /Triage proposal|Nothing is applied/);
  const none = text(await render("/src/views/Brief.svelte", { brief: { ...fx.brief, proposal: null }, now: NOW }));
  assert.doesNotMatch(none, /Triage proposes/);
  const one = text(await render("/src/views/Brief.svelte", { brief: { ...fx.brief, proposal: { count: 1, verbs: { archive: 1 } } }, now: NOW }));
  assert.match(one, /Triage proposes to archive 1 message\./);
});

test("the brief counts what Iris handled without you and links the list (R2-09)", async () => {
  const brief = { ...fx.brief, handled: { count: 504, verbs: { archive: 500 } } };
  const html = await render("/src/views/Brief.svelte", { brief, status: { ...fx.status, mode: "apply" }, now: NOW, poll: 0 });
  const t = text(html);
  assert.match(t, /Handled without you 504 4 hidden from this brief, 500 messages that your rules decided\. See what and why/);
  assert.match(html, /href="#\/handled"/);
  // A rule changed mail: the footer does not say that Iris changed nothing, also with an old status.
  assert.doesNotMatch(t, /It changed nothing/);
  assert.doesNotMatch(text(await render("/src/views/Brief.svelte", { brief, now: NOW })), /It changed nothing/);
  // Only hidden items: one part, and the observe footer stays.
  const only = text(await render("/src/views/Brief.svelte", { brief: { ...fx.brief, handled: { count: 1, verbs: {} }, hidden: 1 }, now: NOW }));
  assert.match(only, /Handled without you 1 1 hidden from this brief\. See what and why/);
  assert.match(only, /Iris only observes\. It changed nothing in your mailbox\./);
  // Nothing handled: no section.
  const none = text(await render("/src/views/Brief.svelte", { brief: { ...fx.brief, handled: { count: 0, verbs: {} }, hidden: 0 }, now: NOW }));
  assert.doesNotMatch(none, /Handled without you/);
  // Apply mode: Iris does not say that it only observes.
  const apply = text(await render("/src/views/Brief.svelte", { brief: { ...fx.brief, handled: { count: 0, verbs: {} }, hidden: 0 }, status: { ...fx.status, mode: "apply" }, now: NOW, poll: 0 }));
  assert.doesNotMatch(apply, /only observes/);
});

test("the fixtures give one handled state, and each hidden item opens", async () => {
  // Observe mode: no rule decides an action, so the brief count is the hidden count, and the list agrees.
  assert.equal(fx.status.mode, "observe");
  const list = fx.answer("/handled?limit=50");
  const hidden = fx.answer("/brief/hidden?limit=50");
  const decided = list.actions.reduce((n, a) => n + a.messages, 0);
  assert.equal(list.count, 0);
  assert.equal(fx.brief.handled.count, fx.brief.hidden + decided);
  assert.equal(hidden.count, fx.brief.hidden);
  assert.deepEqual(fx.brief.handled.verbs, {});
  // The link of each hidden line resolves to its item.
  const html = await render("/src/views/Handled.svelte", { changes: list, hidden });
  for (const item of hidden.items) {
    assert.match(html, new RegExp(`href="#/item/${item.id}"`));
    assert.equal(fx.answer(`/items/${item.id}`)?.id, item.id);
  }
});

test("the handled view lists each change and each hidden item with its reason", async () => {
  const hidden = { count: fx.hidden.length, items: fx.hidden, next: "c1" };
  const html = await render("/src/views/Handled.svelte", { changes: { ...fx.decided, next: "9" }, hidden });
  const t = text(html);
  assert.match(t, /Decided by your rules 1 Archive 500 messages Rule 5 \(Archive old newsletters\) allows archive\. Done, /);
  assert.match(t, /Hidden from your brief 4/);
  assert.equal((t.match(/Hidden by rule 2: Hide Acme Deals/g) ?? []).length, 4);
  // Each hidden item opens as usual.
  for (const item of fx.hidden) assert.match(html, new RegExp(`href="#/item/${item.id}"`));
  // A next page of each list.
  assert.equal((html.match(/>Show more</g) ?? []).length, 2);
  // The reason of the author when the decision has none, and the state in words.
  const plain = { ...fx.decided.actions[0], reason: null, state: "approved", at: null };
  const t2 = text(await render("/src/views/Handled.svelte", { changes: { count: 1, actions: [plain], next: null }, hidden: { count: 0, items: [], next: null } }));
  assert.match(t2, /Feed items that you did not open in 30 days\. Approved, waits to apply/);
  assert.doesNotMatch(t2, /Hidden from your brief|Show more/);
  // Each later state of a rule decision says what happened.
  for (const [state, words] of [["failed", /Failed\. Some messages can have changed/], ["undone", /Undone/], ["denied", /Denied/], ["held", /On hold/]]) {
    const one = text(await render("/src/views/Handled.svelte", { changes: { count: 1, actions: [{ ...plain, state }], next: null }, hidden: { count: 0, items: [], next: null } }));
    assert.match(one, words, state);
  }
  // Nothing: one sentence and a link to the rules.
  const none = await render("/src/views/Handled.svelte", { changes: { count: 0, actions: [], next: null }, hidden: { count: 0, items: [], next: null } });
  assert.match(text(none), /Iris has handled nothing without you yet\./);
  assert.match(none, /href="#\/rules"/);
});

test("a stranger conversation shows once, under strangers", async () => {
  // The API gives it only under strangers (an API from before the stranger gate).
  const brief = { ...fx.brief, conversations: { count: 0, items: [] }, strangers: { count: 1, items: [item("v-88")] }, gate: undefined };
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
  // The raw source needs the sign-in token, so a plain link cannot work: a button fetches it.
  assert.match(html, /<button[^>]*class="raw[^"]*"[^>]*>Download the original</);
  assert.doesNotMatch(html, /href="\/api\/messages\/m-55210\/raw"/);
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
  // m-55210 has a fixed head: the head of its summary.
  assert.match(source, /^From: Priya Raman <priya@northwind\.example>\r\n/);
  assert.match(source, /\r\nSubject: Q4 numbers for the board\r\n/);
  assert.equal(fx.raw("m-0"), undefined);
});

test("the measures page opens with a sentence, the bars of 14 days and one line for each measure", async () => {
  const now = new Date(2026, 10, 16, 9, 5);
  const t = text(await render("/src/views/Measures.svelte", { measures: fx.measures, now }));
  assert.match(t, /You opened 2 source mails today\. One measure misses its target: Days you used Iris\./);
  assert.match(t, /Nothing leaves it/);
  assert.match(t, /Days you used Iris 12 of 14/);
  assert.match(t, /Time to the first brief 6 min From the connect step on 3 Nov/);
  assert.match(t, /Differences from Gmail 0 Last check on 16 Nov/);
  assert.match(t, /Memory of the engine 31 MB With 18,402 messages/);
  assert.match(t, /Changes you approved as proposed 47 of 50 94%/);
  assert.match(t, /Changes that you undid 1 of 48 2%/);
  assert.match(t, /Sends without review after hostile mail Not yet/);
  assert.match(t, /1\.8 a day on the 12 days that you used Iris/);
  const html = await render("/src/views/Measures.svelte", { measures: fx.measures, now });
  // 14 bars: 12 days of use and 2 dotted days with no use. Today is the primary bar.
  assert.equal((html.match(/<span class="[^"]*\boff\b/g) ?? []).length, 2);
  assert.equal((html.match(/<span class="[^"]*\btoday\b/g) ?? []).length, 1);
  assert.match(html, /aria-label="Source mail opened each day: 3 Nov, 4; [^"]*6 Nov, no use;[^"]*16 Nov, 2\."/);
});

test("each measure says when it has no data, and the sentence names a missed target", async () => {
  const { rows, lead, bars, localDay } = await load("/src/measures.ts");
  const now = new Date(2026, 10, 16, 23, 59);
  const empty = { days: [], first_brief: null, mirror: [], memory: { rss_kib: null, messages: 0 }, approvals: { decided: 0, approved: 0 }, undo: { applied: 0, undone: 0 } };
  assert.deepEqual(rows(empty, now).map((r) => r.tone), ["none", "none", "none", "none", "none", "none", "none"]);
  assert.deepEqual(lead(empty, now), { opens: 0, text: "" });
  assert.equal(localDay(now), "2026-11-16");
  assert.equal(bars(empty, now)[0].day, "2026-11-03");

  // Iris started 3 days ago: the days before do not count as days without use.
  const young = { ...empty, days: [{ day: "2026-11-16", opens: 1 }, { day: "2026-11-14", opens: 0 }] };
  assert.deepEqual(rows(young, now)[0], { name: "Days you used Iris", value: "2 of 3", detail: "In the last 3 days. Target: each day.", tone: "missed" });
  assert.equal(lead(young, now).text, "One measure misses its target: Days you used Iris.");

  const slow = { ...empty, first_brief: { connect: "2026-11-03T08:00:00Z", brief: "2026-11-03T08:10:01Z", seconds: 601 }, memory: { rss_kib: 51_201, messages: 9 } };
  const r = rows(slow, now);
  assert.equal(r[1].value, "11 min");
  assert.equal(r[1].tone, "missed");
  assert.equal(r[3].tone, "missed");
  assert.equal(lead(slow, now).text, "2 measures miss their target.");
  const fast = { ...empty, first_brief: { connect: "2026-11-03T08:00:00Z", brief: "2026-11-03T08:10:00Z", seconds: 600 }, memory: { rss_kib: 51_200, messages: 100_000 } };
  assert.deepEqual(rows(fast, now).slice(1, 4).map((x) => x.tone), ["met", "none", "met"]);
  // A store older than the measure: the time is not known, and it has no target tone.
  const old = rows({ ...empty, first_brief: { connect: null, brief: null, seconds: null } }, now)[1];
  assert.deepEqual([old.value, old.tone], ["Not known", "none"]);
  // Under 50 MB with fewer than 100,000 messages does not show the target as met.
  const tone = (rss_kib, messages) => rows({ ...empty, memory: { rss_kib, messages } }, now)[3].tone;
  assert.equal(tone(30_000, 99_999), "flat");
  assert.equal(tone(51_201, 100_000), "missed");
  assert.equal(tone(61_440, 200_000), "flat");
  assert.equal(tone(51_200, 200_000), "met");

  // A check with differences misses; a check that stopped early cannot meet the target.
  const check = (differences, complete) => ({ ...empty, mirror: [{ account: "a@example.com", checked: "2026-11-16T07:00:00Z", differences, complete }] });
  assert.equal(rows(check(3, true), now)[2].tone, "missed");
  assert.equal(rows(check(0, false), now)[2].tone, "flat");
  assert.equal(rows(check(null, true), now)[2].tone, "flat");
  assert.equal(rows(check(0, true), now)[2].tone, "met");
  // Invariant: the target is met only when each account has a complete check with no
  // difference. An account with no check keeps it from "met".
  const none = { account: "b@example.com", checked: null, differences: null, complete: false };
  const mixed = { ...check(0, true), mirror: [...check(0, true).mirror, none] };
  assert.equal(rows(mixed, now)[2].tone, "flat");
  assert.match(rows(mixed, now)[2].detail, /^1 account has no check yet\. Last check on/);
  assert.equal(rows({ ...empty, mirror: [none] }, now)[2].tone, "none");
  assert.equal(rows({ ...mixed, mirror: [{ ...mixed.mirror[0], differences: 2 }, none] }, now)[2].tone, "missed");
});

test("each request while the person is active sends the local day, and a poll of an idle tab does not", async () => {
  const { get } = await load("/src/api.ts");
  const { localDay, noteInput, ACTIVE_MS } = await load("/src/measures.ts");
  const saved = globalThis.fetch;
  const sent = [];
  globalThis.fetch = async (url, init) => {
    sent.push([url, init?.headers?.["x-iris-day"]]);
    return new Response("{}", { status: 200 });
  };
  try {
    // Invariant: a request sends the day only within ACTIVE_MS of an input of the person.
    noteInput(Date.now() - ACTIVE_MS - 1000);
    await get("notices");
    noteInput();
    await get("status");
    await get("messages/m-1");
    assert.deepEqual(sent, [
      ["/api/notices", undefined],
      ["/api/status", localDay()],
      ["/api/messages/m-1", localDay()],
    ]);
  } finally {
    globalThis.fetch = saved;
  }
});

test("due dates read as words, with a tone for soon and late", async () => {
  const { due } = await load("/src/format.ts");
  const at = (date) => ({ ...item("c-311"), due: { date } });
  const now = new Date(2026, 10, 16, 12);
  assert.deepEqual(due(at("2026-11-17"), now), { text: "due tomorrow", tone: "soon" });
  assert.deepEqual(due(at("2026-11-26"), now), { text: "due in 10 days", tone: "" });
  assert.deepEqual(due(at("2026-11-14"), now), { text: "was due 2 days ago", tone: "late" });
  // A due time earlier today has passed.
  const time = (value) => ({ ...item("c-311"), due: { at: value } });
  assert.deepEqual(due(time(new Date(2026, 10, 16, 9).toISOString()), now), { text: "was due today", tone: "late" });
  assert.deepEqual(due(time(new Date(2026, 10, 16, 18).toISOString()), now), { text: "due today", tone: "soon" });
});

test("a conversation line names a party that is not you", async () => {
  const { other } = await load("/src/format.ts");
  const talk = item("v-88");
  const sam = talk.participants.find((p) => p.kind !== "self");
  assert.deepEqual(other({ ...talk, latest: { ...talk.latest, from: { kind: "self" } } }), sam);
  assert.deepEqual(other(talk), talk.latest.from);
});

test("the client trades the code of the sign-in address for a token, and sends the token", async () => {
  const { get, signIn, download, SIGN_IN_FAILED } = await load("/src/api.ts");
  const saved = globalThis.fetch;
  const calls = [];
  const memory = new Map();
  const store = { getItem: (k) => memory.get(k) ?? null, setItem: (k, v) => memory.set(k, v), removeItem: (k) => memory.delete(k) };
  const loc = { hash: "#code=abc%2B1", pathname: "/", search: "" };
  const urls = [];
  const hist = { replaceState: (_s, _t, url) => urls.push(url) };
  try {
    globalThis.fetch = async (url, init = {}) => {
      calls.push({ url, init });
      if (url === "/api/session") return new Response(JSON.stringify({ token: "t-1" }), { status: 200 });
      return new Response(JSON.stringify({ ok: 1 }), { status: 200 });
    };
    await signIn(loc, hist, store);
    // The code leaves the address bar before the exchange, and the browser never sends it to a server.
    assert.deepEqual(urls, ["/#/"]);
    assert.equal(calls[0].init.method, "POST");
    assert.deepEqual(JSON.parse(calls[0].init.body), { code: "abc+1" });
    assert.equal(memory.get("iris.token"), "t-1");
    await get("status");
    assert.equal(calls[1].init.headers.authorization, "Bearer t-1");
    // A used or wrong code gives 403: say so, and keep no token.
    memory.clear();
    globalThis.fetch = async () => new Response(JSON.stringify({ error: "used" }), { status: 403 });
    await assert.rejects(signIn({ ...loc }, hist, store), { message: SIGN_IN_FAILED });
    assert.equal(memory.size, 0);
    // With no code in the address, nothing is sent.
    calls.length = 0;
    globalThis.fetch = async (url, init) => (calls.push({ url, init }), new Response("{}"));
    await signIn({ hash: "#/brief", pathname: "/", search: "" }, hist, store);
    assert.equal(calls.length, 0);
    // The download of the original sends the token too.
    globalThis.fetch = async () => new Response(JSON.stringify({ token: "t-2" }), { status: 200 });
    await signIn(loc, hist, store);
    globalThis.fetch = async (url, init) => (calls.push({ url, init }), new Response("From: a@example.com\r\n\r\nHi", { status: 200 }));
    const saved_ = [];
    await download("m-55210", (blob, name) => saved_.push([blob, name]));
    assert.equal(calls.at(-1).url, "/api/messages/m-55210/raw");
    assert.equal(calls.at(-1).init.headers.authorization, "Bearer t-2");
    assert.equal(saved_[0][1], "m-55210.eml");
    assert.equal(await saved_[0][0].text(), "From: a@example.com\r\n\r\nHi");
  } finally {
    globalThis.fetch = saved;
  }
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
  assert.deepEqual(parse("#/brief/gate"), { view: "gate", declined: false });
  assert.deepEqual(parse("#/brief/declined"), { view: "gate", declined: true });
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

test("a next page adds only items that the list does not show yet", async () => {
  // Keyset paging can give an item again when its sort key moves between two pages.
  const { addPage } = await load("/src/format.ts");
  const a = item("c-311");
  const b = { ...a, id: "c-2" };
  assert.deepEqual(addPage([a], [a, b]).map((i) => i.id), ["c-311", "c-2"]);
});

test("a mail renders the engine shape: header text for To and Cc, and fields that can be null", async () => {
  const message = { ...fx.message("m-55210"), to: "Sam Ortiz <sam@example.com>, b@example.com", cc: null, date: null, subject: null, from: { name: null, address: "a@example.com" } };
  const t = text(await render("/src/views/Mail.svelte", { message }));
  assert.match(t, /No subject/);
  assert.match(t, /a@example\.com/);
  assert.match(t, /To Sam Ortiz &lt;sam@example\.com>, b@example\.com/);
  assert.doesNotMatch(t, /copy to/);
  const both = text(await render("/src/views/Mail.svelte", { message: { ...message, to: null, cc: "c@example.com" } }));
  assert.match(both, /To no one, copy to c@example\.com/);
});

test("a mail that Iris has not read yet keeps the download of the original", async () => {
  const html = await render("/src/views/Mail.svelte", { message: { ...fx.message("m-55210"), text: null, gone: false } });
  // During the first copy, the original can be missing too, so the page does not promise it.
  assert.match(text(html), /Iris has not read this mail yet\. If Iris copied the original, you can download it\./);
  assert.match(html, /<button[^>]*class="raw[^"]*"[^>]*>Download the original</);
  assert.doesNotMatch(html, /<pre/);
});


test("a sign-in that does not reach the engine, or gets a server error, keeps the code for a reload", async () => {
  const { signIn, SIGN_IN_FAILED, SIGN_IN_RETRY } = await load("/src/api.ts");
  const saved = globalThis.fetch;
  const memory = new Map();
  const store = { getItem: (k) => memory.get(k) ?? null, setItem: (k, v) => memory.set(k, v), removeItem: (k) => memory.delete(k) };
  const hist = { replaceState: () => {} };
  const sent = [];
  try {
    globalThis.fetch = async () => { throw new TypeError("Failed to fetch"); };
    await assert.rejects(signIn({ hash: "#code=c-9", pathname: "/", search: "" }, hist, store));
    // The reload has no code in the address, and sends the kept code.
    globalThis.fetch = async (url, init) => (sent.push(JSON.parse(init.body)), new Response(JSON.stringify({ token: "t-9" })));
    await signIn({ hash: "#/", pathname: "/", search: "" }, hist, store);
    assert.deepEqual(sent, [{ code: "c-9" }]);
    assert.equal(memory.get("iris.token"), "t-9");
    assert.equal(memory.has("iris.code"), false);
    // A refused code is not kept: the engine used or refused it.
    globalThis.fetch = async () => new Response(JSON.stringify({ error: "used" }), { status: 403 });
    await assert.rejects(signIn({ hash: "#code=c-10", pathname: "/", search: "" }, hist, store), { message: SIGN_IN_FAILED });
    assert.equal(memory.has("iris.code"), false);
    // A server error is not a refusal: keep the code, and tell the person to load the page again, not to restart.
    globalThis.fetch = async () => new Response(JSON.stringify({ error: "store" }), { status: 500 });
    await assert.rejects(signIn({ hash: "#code=c-11", pathname: "/", search: "" }, hist, store), { message: SIGN_IN_RETRY });
    assert.equal(memory.get("iris.code"), "c-11");
    assert.doesNotMatch(SIGN_IN_RETRY, /Start iris serve/);
  } finally {
    globalThis.fetch = saved;
  }
});


test("a sign-in address with a bad escape gives the sign-in error, not a crash", async () => {
  const { signIn, SIGN_IN_FAILED } = await load("/src/api.ts");
  const urls = [];
  let result;
  assert.doesNotThrow(() => (result = signIn({ hash: "#code=%", pathname: "/", search: "" }, { replaceState: (_s, _t, u) => urls.push(u) }, undefined)));
  await assert.rejects(result, { message: SIGN_IN_FAILED });
  assert.deepEqual(urls, ["/#/"]);
});

test("only the newest request of a view applies its answer (A, then B, then A)", async () => {
  const { newest } = await load("/src/api.ts");
  const ticket = newest();
  const a1 = ticket();
  const b = ticket();
  const a2 = ticket();
  assert.equal(a1(), false);
  assert.equal(b(), false);
  assert.equal(a2(), true);
});

test("one access state holds the sign-in and engine problem of every call, and a good answer clears it", async () => {
  const { get, signIn, SIGN_IN, SIGN_IN_FAILED, UNREACHABLE } = await load("/src/api.ts");
  const { access } = await load("/src/access.svelte.ts");
  const saved = globalThis.fetch;
  const ok = async () => new Response(JSON.stringify({ ok: 1 }));
  try {
    // Start from a good sign-in: an earlier test leaves a refused one.
    globalThis.fetch = async () => new Response(JSON.stringify({ token: "t-0" }));
    await signIn({ hash: "#code=z", pathname: "/", search: "" }, undefined, undefined);
    // A token that the engine does not know (for example after a restart of iris serve).
    globalThis.fetch = async () => new Response(JSON.stringify({ error: "no token" }), { status: 401 });
    await assert.rejects(get("status"), { message: SIGN_IN });
    assert.equal(access.problem, SIGN_IN);
    globalThis.fetch = ok;
    await get("status");
    assert.equal(access.problem, "");
    // The engine does not answer: a poll gets the same state as a first load.
    globalThis.fetch = async () => { throw new TypeError("Failed to fetch"); };
    await assert.rejects(get("brief"), { message: UNREACHABLE });
    assert.equal(access.problem, UNREACHABLE);
    globalThis.fetch = ok;
    await get("brief");
    assert.equal(access.problem, "");
    // A refused code.
    globalThis.fetch = async () => new Response("{}", { status: 403 });
    await assert.rejects(signIn({ hash: "#code=x", pathname: "/", search: "" }, undefined, undefined), { message: SIGN_IN_FAILED });
    assert.equal(access.problem, SIGN_IN_FAILED);
    // A bad escape.
    access.problem = "";
    await assert.rejects(signIn({ hash: "#code=%", pathname: "/", search: "" }, undefined, undefined));
    assert.equal(access.problem, SIGN_IN_FAILED);
    // A good code clears the problem.
    globalThis.fetch = async () => new Response(JSON.stringify({ token: "t-3" }));
    await signIn({ hash: "#code=y", pathname: "/", search: "" }, undefined, undefined);
    assert.equal(access.problem, "");
  } finally {
    globalThis.fetch = saved;
    access.problem = "";
  }
});

test("each status answer sets the accounts that need sign-in, and the app shows them above every view", async () => {
  const { get } = await load("/src/api.ts");
  const { access } = await load("/src/access.svelte.ts");
  const saved = globalThis.fetch;
  const account = { ...fx.status.accounts[0], state: "needs_sign_in" };
  try {
    globalThis.fetch = async () => new Response(JSON.stringify({ ...fx.status, accounts: [account] }));
    await get("status");
    assert.deepEqual(access.locked, [account.address]);
    // The views start API calls while they render. Keep them pending.
    globalThis.fetch = () => new Promise(() => {});
    for (const hash of ["", "#/setup", "#/brief", "#/mail/m-55210"]) {
      globalThis.location = { hash, pathname: "/", search: "" };
      const t = text(await render("/src/App.svelte"));
      assert.match(t, new RegExp(`Google stopped the sign-in of ${account.address.replace(".", "\\.")}\\. Iris copies no new mail from this account until you sign in again\\.`));
      assert.match(t, /iris account add client\.json/);
    }
    globalThis.fetch = async () => new Response(JSON.stringify(fx.status));
    await get("status");
    assert.deepEqual(access.locked, []);
    globalThis.fetch = () => new Promise(() => {});
    assert.doesNotMatch(text(await render("/src/App.svelte")), /stopped the sign-in/);
  } finally {
    globalThis.fetch = saved;
    delete globalThis.location;
    access.locked = [];
  }
});

test("each status answer sets if there is no account, and the app then shows the setup link above the other views", async () => {
  const { get } = await load("/src/api.ts");
  const { access } = await load("/src/access.svelte.ts");
  const saved = globalThis.fetch;
  try {
    globalThis.fetch = async () => new Response(JSON.stringify({ ...fx.status, accounts: [] }));
    await get("status");
    assert.equal(access.none, true);
    globalThis.fetch = () => new Promise(() => {});
    for (const hash of ["#/commitments", "#/records", "#/brief/you_owe", "#/mail/m-55210", "#/measures"]) {
      globalThis.location = { hash, pathname: "/", search: "" };
      assert.match(text(await render("/src/App.svelte")), /Iris has no account yet\. Set up Iris to copy your mail\./, hash);
    }
    globalThis.fetch = async () => new Response(JSON.stringify(fx.status));
    await get("status");
    assert.equal(access.none, false);
    globalThis.fetch = () => new Promise(() => {});
    globalThis.location = { hash: "#/records", pathname: "/", search: "" };
    assert.doesNotMatch(text(await render("/src/App.svelte")), /no account yet/);
  } finally {
    globalThis.fetch = saved;
    delete globalThis.location;
    access.none = false;
  }
});

test("a sign-in address opened in a tab that shows Iris already signs in, then loads the page again", async () => {
  // A paste of the address in the same tab changes only the hash, so the page does not load again by itself.
  const { followSignIn } = await load("/src/api.ts");
  const saved = globalThis.fetch;
  let reloads = 0;
  const hist = { replaceState: () => {} };
  try {
    globalThis.fetch = async () => new Response(JSON.stringify({ token: "t-5" }));
    assert.equal(await followSignIn({ hash: "#code=k", pathname: "/", search: "" }, hist, undefined, () => reloads++), true);
    assert.equal(reloads, 1);
    // A refused code does not load the page again: the access state shows the reason.
    globalThis.fetch = async () => new Response("{}", { status: 403 });
    assert.equal(await followSignIn({ hash: "#code=k", pathname: "/", search: "" }, hist, undefined, () => reloads++), false);
    // No engine: no reload, so the page does not load again and again.
    globalThis.fetch = async () => { throw new TypeError("Failed to fetch"); };
    assert.equal(await followSignIn({ hash: "#code=k", pathname: "/", search: "" }, hist, undefined, () => reloads++), false);
    // Another hash is not a sign-in.
    assert.equal(await followSignIn({ hash: "#/brief", pathname: "/", search: "" }, hist, undefined, () => reloads++), false);
    assert.equal(reloads, 1);
  } finally {
    globalThis.fetch = saved;
  }
});

test("the brief and setup show the first sync while any account still copies", async () => {
  const ready = { ...fx.status.accounts[0], id: 1, address: "a@example.com" };
  const copying = { ...fx.status.accounts[0], id: 2, address: "b@example.com", mirror_ready_at: null, sync: { kind: "first", step: 2 } };
  const status = { accounts: [ready, copying], messages: { total: 84212, fetched: 20000, understood: 500, gone: 0 } };
  const brief = { ...fx.brief, oldest_fetched: "2026-09-01T00:00:00Z" };
  assert.match(text(await render("/src/views/Brief.svelte", { brief, status, now: NOW, poll: 0 })), /Iris still copies your mailbox: 20,000 of 84,212 messages from all accounts\. This brief covers the mail since/);
  assert.match(text(await render("/src/views/Setup.svelte", { status, poll: 0 })), /Iris copies b@example\.com/);
  const done = { ...status, accounts: [ready, { ...copying, mirror_ready_at: "2026-11-16T08:00:00Z" }] };
  assert.doesNotMatch(text(await render("/src/views/Brief.svelte", { brief, status: done, now: NOW, poll: 0 })), /Iris still copies/);
});

test("the brief and setup say that the copy stopped for an account that needs sign-in", async () => {
  // Spec 1 section 4.7: a refused sign-in stops the sync of the account.
  const stopped = { ...fx.status.accounts[0], address: "b@example.com", mirror_ready_at: null, state: "needs_sign_in" };
  const status = { accounts: [stopped], messages: { total: 84212, fetched: 20000, understood: 500, gone: 0 } };
  const brief = { ...fx.brief, oldest_fetched: "2026-09-01T00:00:00Z" };
  const b = text(await render("/src/views/Brief.svelte", { brief, status, now: NOW, poll: 0 }));
  assert.doesNotMatch(b, /still copies|copy continues/);
  assert.match(b, /Iris copied 20,000 of 84,212 messages, then Google stopped the sign-in\. This brief covers the mail since/);
  const early = text(await render("/src/views/Brief.svelte", { brief: { ...brief, oldest_fetched: null }, status, now: NOW, poll: 0 }));
  assert.doesNotMatch(early, /copy continues/);
  const t = text(await render("/src/views/Setup.svelte", { status, poll: 0 }));
  assert.doesNotMatch(t, /Iris copies|copy continues/);
  assert.match(t, /The copy of b@example\.com stopped\. Iris copied 20,000 of 84,212 messages\./);
  const { copying } = await load("/src/format.ts");
  assert.deepEqual(copying(status), []);
});

test("setup and the brief say that the counts of two or more accounts are totals", async () => {
  // The API counts the messages of all accounts together (crates/iris-api/README.md, /api/status).
  const ready = { ...fx.status.accounts[0], id: 1, address: "a@example.com" };
  const copying = { ...fx.status.accounts[0], id: 2, address: "b@example.com", mirror_ready_at: null };
  const status = { accounts: [ready, copying], messages: { total: 11000, fetched: 10100, understood: 500, gone: 0 } };
  assert.match(text(await render("/src/views/Setup.svelte", { status, poll: 0 })), /10,100 of 11,000 messages copied from all accounts\./);
  const stopped = { ...status, accounts: [ready, { ...copying, state: "needs_sign_in" }] };
  assert.match(text(await render("/src/views/Setup.svelte", { status: stopped, poll: 0 })), /10,100 of 11,000 messages from all accounts/);
  assert.match(text(await render("/src/views/Brief.svelte", { brief: fx.brief, status, now: NOW, poll: 0 })), /10,100 of 11,000 messages from all accounts\./);
  // With one account, the counts are of that account.
  const one = { ...status, accounts: [copying] };
  assert.doesNotMatch(text(await render("/src/views/Setup.svelte", { status: one, poll: 0 })), /all accounts/);
});

test("a record field shows a party as a person, also in a list and with no name", async () => {
  const { field } = await load("/src/format.ts");
  const sender = { kind: "sender", sender: "p-3", address: "x@example.com" };
  assert.equal(field(sender), "x@example.com");
  assert.equal(field([{ kind: "self" }, sender, { kind: "sender", name: "Acme Legal", address: "legal@acme.example" }]), "You, x@example.com, Acme Legal");
  assert.equal(field({ value: 12.5, currency: "USD" }), "12.5 USD");
});

test("an old 401 that ends after a new sign-in keeps the new token", async () => {
  const { get, signIn } = await load("/src/api.ts");
  const saved = globalThis.fetch;
  const sent = [];
  let answerOld;
  try {
    globalThis.fetch = async () => new Response(JSON.stringify({ token: "old" }));
    await signIn({ hash: "#code=a", pathname: "/", search: "" }, undefined, undefined);
    globalThis.fetch = (url, init) => new Promise((resolve) => (answerOld = () => resolve(new Response("{}", { status: 401 }))));
    const old = get("status").catch(() => {});
    await new Promise((r) => setTimeout(r, 0));
    globalThis.fetch = async () => new Response(JSON.stringify({ token: "new" }));
    await signIn({ hash: "#code=b", pathname: "/", search: "" }, undefined, undefined);
    answerOld();
    await old;
    globalThis.fetch = async (url, init) => (sent.push(init.headers.authorization), new Response(JSON.stringify({ accounts: [] })));
    await get("status");
    assert.deepEqual(sent, ["Bearer new"]);
  } finally {
    globalThis.fetch = saved;
  }
});

test("a download of an original that Iris has not copied yet says so", async () => {
  const { download, NOT_COPIED } = await load("/src/api.ts");
  const saved = globalThis.fetch;
  try {
    globalThis.fetch = async () => new Response(JSON.stringify({ error: "not found" }), { status: 404 });
    await assert.rejects(download("m-7", () => {}), { message: NOT_COPIED });
  } finally {
    globalThis.fetch = saved;
  }
});

test("each brief section that shows has its All link, also with fewer than 8 items", async () => {
  const html = await render("/src/views/Brief.svelte", { brief: fx.brief, now: NOW, poll: 0 });
  assert.match(html, /href="#\/brief\/you_owe"[^>]*>All 2</);
  // A Signals section of live login codes only: the codes show above the sections, and the section keeps its link.
  const codes = { items: [item("s-901")], count: 1 };
  const only = await render("/src/views/Brief.svelte", { brief: { ...fx.brief, signals: codes }, now: NOW, poll: 0 });
  assert.match(only, /href="#\/brief\/signals"[^>]*>All 1</);
});

test("the signals of an open brief leave at their expiry, and the count follows", async () => {
  const { signals } = await load("/src/format.ts");
  const code = item("s-901");
  const notice = { ...code, id: "s-2", code: undefined, title: "Delivery window", expires: "2026-11-16T10:00:00Z" };
  const lasting = { ...code, id: "s-3", code: undefined, title: "Account notice", expires: undefined };
  const section = { count: 3, items: [code, notice, lasting] };
  const open = signals(section, NOW);
  assert.deepEqual(open.codes.map((i) => i.id), ["s-901"]);
  assert.deepEqual(open.rows.map((i) => i.id), ["s-2", "s-3"]);
  assert.equal(open.count, 3);
  // The view wakes at the first expiry of any signal.
  assert.equal(open.next, code.expires);
  const later = signals(section, new Date(Date.parse(code.expires) + 1000));
  assert.deepEqual(later.codes, []);
  assert.deepEqual(later.rows.map((i) => i.id), ["s-2", "s-3"]);
  assert.equal(later.count, 2);
  assert.equal(later.next, notice.expires);
  const last = signals(section, new Date("2026-11-16T10:00:01Z"));
  assert.deepEqual(last.rows.map((i) => i.id), ["s-3"]);
  assert.equal(last.count, 1);
  assert.equal(last.next, undefined);
});

test("a signal page shows its live code first, then the quote of its mail", async () => {
  // Spec 1 section 6.6: the code is the reason to open the page, so it comes before the quote.
  const t = text(await render("/src/views/Item.svelte", { item: item("s-901"), now: NOW }));
  const code = t.indexOf("482913");
  const quote = t.indexOf("From the mail that made this item");
  assert.ok(code >= 0 && (quote < 0 || code < quote));
});

test("the brief loads each page of Signals when the API cuts the section, so a live code is not lost", async () => {
  const { loadBrief } = await load("/src/api.ts");
  const saved = globalThis.fetch;
  const code = item("s-901");
  const others = Array.from({ length: 2 }, (_, n) => ({ ...code, id: `s-${n}`, code: undefined }));
  const asked = [];
  try {
    globalThis.fetch = async (url) => {
      asked.push(url);
      if (url === "/api/brief") return new Response(JSON.stringify({ ...fx.brief, signals: { count: 3, items: [others[0]] } }));
      if (url === "/api/brief/signals?limit=200") return new Response(JSON.stringify({ count: 3, items: [others[0], others[1]], next: "c1" }));
      if (url === "/api/brief/signals?limit=200&after=c1") return new Response(JSON.stringify({ count: 3, items: [code], next: null }));
      return new Response("{}", { status: 404 });
    };
    const brief = await loadBrief();
    assert.deepEqual(brief.signals.items.map((i) => i.id), ["s-0", "s-1", "s-901"]);
    // A complete section needs no more calls.
    asked.length = 0;
    globalThis.fetch = async (url) => (asked.push(url), new Response(JSON.stringify(fx.brief)));
    await loadBrief();
    assert.deepEqual(asked, ["/api/brief"]);
  } finally {
    globalThis.fetch = saved;
  }
});

test("the full Signals section leaves out a signal after its expiry, and wakes at the next one", async () => {
  const { nextExpiry } = await load("/src/format.ts");
  const code = item("s-901");
  const later = { ...code, id: "s-2", code: undefined, title: "Delivery window", expires: "2026-11-16T10:00:00Z" };
  const page = { items: [code, later], next: null };
  const props = { title: "Signals", page, more: "brief/signals?limit=50", cursor: "after", expiring: true };
  const open = text(await render("/src/views/List.svelte", { ...props, now: NOW }));
  assert.match(open, /Login code for Example Bank/);
  assert.match(open, /Delivery window/);
  const after = text(await render("/src/views/List.svelte", { ...props, now: new Date(Date.parse(code.expires) + 1000) }));
  assert.doesNotMatch(after, /Login code for Example Bank/);
  assert.match(after, /Delivery window/);
  assert.equal(nextExpiry([code, later], NOW), code.expires);
  assert.equal(nextExpiry([later], new Date("2026-11-16T10:00:01Z")), undefined);
  // A list of a form keeps its items: it shows the history of the form.
  const form = text(await render("/src/views/List.svelte", { title: "Signals", page, now: new Date(Date.parse(code.expires) + 1000) }));
  assert.match(form, /Login code for Example Bank/);
  // The time left of each line follows the clock of the list.
  const soon = text(await render("/src/views/List.svelte", { title: "Signals", page: { items: [later], next: null }, now: new Date(Date.parse(later.expires) - 5 * 60_000) }));
  assert.match(soon, /in 5 minutes/);
});

test("the brief shows each bundle as one line and counts the hidden items (spec 2 section 6.4)", async () => {
  const feed = (n, sender, name) => ({ ...item("f-5001"), id: `f-${n}`, publisher: { kind: "sender", sender, name } });
  const items = [feed(1, "p-1", "Mail Weekly"), feed(2, "p-1", "Mail Weekly"), feed(3, "p-2", "Rust Digest"), feed(4, "p-3", "The Batch"), feed(5, "p-4", "City News"), feed(6, "p-5", "News")];
  const bundles = [
    { rule: 3, name: "Bundle the feed", count: 6, items },
    { rule: 8, name: "Bundle the school", count: 40, items: items.slice(0, 2) },
  ];
  const html = await render("/src/views/Brief.svelte", { brief: { ...fx.brief, bundles, hidden: 23 }, now: NOW });
  const t = text(html);
  assert.match(t, /Bundled by your rules 46/);
  // Each sender once, at most 3 names, and the count of the others when the API gave every item.
  assert.match(t, /Bundle the feed , 6 items From Mail Weekly, Rust Digest, The Batch and 2 others/);
  // Not every item: the count of the other senders is not known.
  assert.match(t, /Bundle the school , 40 items From Mail Weekly and others/);
  assert.match(html, /href="#\/brief\/bundle\/3"/);
  assert.match(html, /href="#\/brief\/bundle\/8"/);
  // One hidden item, no bundle: no bundle section.
  const one = text(await render("/src/views/Brief.svelte", { brief: { ...fx.brief, bundles: [], hidden: 1 }, now: NOW }));
  assert.doesNotMatch(one, /Bundled by your rules/);
  // An API from before bundles and R2-09: no section and no handled count.
  const old = text(await render("/src/views/Brief.svelte", { brief: { ...fx.brief, bundles: undefined, hidden: undefined, handled: undefined }, now: NOW }));
  assert.doesNotMatch(old, /Bundled by your rules|Handled without you/);
});

test("a bundle has a route, and a bad rule number opens the brief", async () => {
  const { parse, href } = await load("/src/route.ts");
  assert.deepEqual(parse("#/brief/bundle/3"), { view: "bundle", rule: 3 });
  assert.equal(href.bundle(3), "#/brief/bundle/3");
  for (const bad of ["#/brief/bundle/", "#/brief/bundle/0", "#/brief/bundle/03", "#/brief/bundle/x", "#/brief/bundle/-1"]) {
    assert.notEqual(parse(bad).view, "bundle", bad);
  }
});

test("the Records view counts from /api/records, so a record type that a rule hides still shows, and a bundle page reads its bundle", async () => {
  const saved = globalThis.fetch;
  const asked = [];
  // Keep each call pending: the test checks only the address of each call.
  globalThis.fetch = (url) => {
    asked.push(String(url));
    return new Promise(() => {});
  };
  try {
    for (const hash of ["#/records", "#/records/receipt", "#/brief/bundle/3"]) {
      asked.length = 0;
      globalThis.location = { hash, pathname: "/", search: "" };
      await render("/src/App.svelte");
      // The call waits for the sign-in state first.
      await new Promise((r) => setTimeout(r, 50));
      const api = asked.filter((u) => !/\/api\/(status|session)/.test(u));
      if (hash.startsWith("#/records")) {
        assert.ok(api.includes("/api/records"), `${hash}: ${api}`);
        assert.ok(!api.some((u) => u.startsWith("/api/brief")), `${hash}: ${api}`);
      } else {
        assert.ok(api.includes("/api/brief/bundle-3?limit=50"), `${hash}: ${api}`);
      }
    }
  } finally {
    globalThis.fetch = saved;
    delete globalThis.location;
  }
});
