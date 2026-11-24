// Answers of the local API, made from the examples of docs/spec/schema/examples.
// The dev server (`pnpm dev --mode fixtures`) and the tests use them. They are not in the build.
import type { Outbox, Action, AskAnswer, DraftView, Brief, GateAnswer, GatePage, Changeset, HandledPage, Item, LedgerEntry, LedgerPage, Job, JobDetail, Matter, MatterList, MatterView, Message, Policy, ReplyStart, Rule, Status, Summary, Target } from "../src/api";
import type { Measures } from "../src/measures";
import youOwe from "../../docs/spec/schema/examples/commitment-you-owe.json" with { type: "json" };
import owedToYou from "../../docs/spec/schema/examples/commitment-owed-to-you.json" with { type: "json" };
import payment from "../../docs/spec/schema/examples/commitment-payment.json" with { type: "json" };
import conversation from "../../docs/spec/schema/examples/conversation.json" with { type: "json" };
import receipt from "../../docs/spec/schema/examples/record-receipt.json" with { type: "json" };
import booking from "../../docs/spec/schema/examples/record-booking.json" with { type: "json" };
import feed from "../../docs/spec/schema/examples/feed.json" with { type: "json" };
import signal from "../../docs/spec/schema/examples/signal-code.json" with { type: "json" };
import matterConfirmed from "../../docs/spec/schema/examples/matter-confirmed.json" with { type: "json" };
import matterProposed from "../../docs/spec/schema/examples/matter-proposed.json" with { type: "json" };

// Two accounts (card R6-02): the work mailbox has the conversation and the commitment owed to you.
const work = new Set(["commitment-owed-to-you", "conversation"]);
export const items = Object.entries({ "commitment-you-owe": youOwe, "commitment-owed-to-you": owedToYou, payment, conversation, receipt, booking, feed, signal }).map(
  ([name, item]) => ({ ...item, model: "iris/1.1", account: work.has(name) ? "a-2" : "a-1" }),
) as unknown as Item[];

const of = (list: Item[]) => ({ count: list.length, items: list });
const byForm = (form: string) => items.filter((i) => i.form === form);

export const status: Status = {
  mode: "observe",
  accounts: [
    {
      id: 1,
      address: "you@example.com",
      provider: "gmail",
      state: null,
      mirror_ready_at: "2026-11-16T08:00:00Z",
      sync: null,
      last_sync: { kind: "incremental", started: "2026-11-16T09:00:00Z", ended: "2026-11-16T09:00:02Z", result: "ok: history 9120, fetched 3, gone 0, failed 0" },
    },
    {
      id: 2,
      address: "you@northwind.example",
      provider: "gmail",
      state: null,
      mirror_ready_at: "2026-11-16T08:30:00Z",
      sync: null,
      last_sync: { kind: "incremental", started: "2026-11-16T09:00:00Z", ended: "2026-11-16T09:00:01Z", result: "ok: history 4410, fetched 1, gone 0, failed 0" },
    },
  ],
  messages: { total: 84212, fetched: 84212, understood: 84212, gone: 0 },
};

const stranger = (id: string, name: string | null, address: string) => ({ kind: "sender" as const, sender: id, ...(name ? { name } : {}), address });
const ask = (id: string, title: string, who: ReturnType<typeof stranger>, due: string | null, message: string, quote: string): Item =>
  ({ ...youOwe, id, title, counterparty: who, due: due ? { date: due, strict: false } : undefined, sources: [{ message, thread: `t-${message.slice(2)}`, role: "origin", quote }] }) as unknown as Item;
const maria = stranger("p-71", "Maria Ruiz", "maria@brightpath.example");
const leo = stranger("p-72", "Leo Park", "leo@quartzlabs.example");
const vendor = stranger("p-73", null, "billing@vendorhub.example");
/** The items of the stranger gate (spec 1 section 6.7): three senders that you do not know. */
export const strangers: Item[] = [
  ask("c-401", "Review the pitch deck before Friday", maria, "2026-11-20", "m-60101", "Could you look at our pitch deck before Friday?"),
  { ...(conversation as unknown as Item), id: "v-402", title: "Intro from Sam Ortiz", participants: [{ kind: "self" }, maria], latest: { at: "2026-11-16T07:55:00Z", from: maria, snippet: "Sam said that you two worked on the same team." } },
  ask("c-403", "Speak at the March meetup", leo, null, "m-60102", "Would you speak at our March meetup?"),
  ask("c-404", "Confirm the address for the invoice", vendor, "2026-11-18", "m-60103", "Please confirm the billing address for the invoice."),
];
export const gate: GatePage = {
  count: 3,
  next: null,
  senders: [
    { id: "p-71", name: "Maria Ruiz", address: "maria@brightpath.example", count: 2, items: [strangers[1], strangers[0]] },
    { id: "p-72", name: "Leo Park", address: "leo@quartzlabs.example", count: 1, items: [strangers[2]] },
    { id: "p-73", name: null, address: "billing@vendorhub.example", count: 1, items: [strangers[3]] },
  ],
};
/** One sender that you ignored at the gate. */
export const ignored: GatePage = {
  count: 1,
  next: null,
  senders: [{ id: "p-74", name: "Dana Brooks", address: "dana@leadgen.example", count: 1, items: [ask("c-405", "Book a call about your Q1 plans", stranger("p-74", "Dana Brooks", "dana@leadgen.example"), null, "m-60001", "Do you have 15 minutes this week?")] }],
};

export const brief: Brief = {
  // Iris prepared a reply for the first item (card R3-09).
  you_owe: of(byForm("commitment").filter((i) => i.direction === "you_owe").map((i, n) => (n ? i : { ...i, ready: ["draft"] }))),
  // Alex is late: Iris prepared a nudge (card R3-07).
  owed_to_you: of(byForm("commitment").filter((i) => i.direction === "owed_to_you").map((i, n) => (n ? i : { ...i, ready: ["draft"] }))),
  conversations: of(byForm("conversation")),
  records: { receipt: 1, booking: 1 },
  // The feed example is bundled by rule 3 ("Bundle the feed"), so it is only in its bundle.
  feed: of([]),
  signals: of(byForm("signal")),
  strangers: of(strangers),
  gate,
  declined: ignored.count,
  hostile: 2,
  not_understood: 1,
  oldest_fetched: null,
  proposal: { count: 447, verbs: { archive: 412, label: 35 } },
  bundles: [{ rule: 3, name: "Bundle the feed", ...of(byForm("feed")) }],
  hidden: 4,
  // Observe mode: no rule approves an action, so only the hidden items count.
  handled: { count: 4, verbs: {} },
  review: { changesets: 2, actions: 4 },
};

/** The items that rule 2 hides (spec 2 section 6.4): the four hidden items of the brief, each with its own ID. */
export const hidden: Item[] = items.slice(0, 4).map((i, n) => ({
  ...i,
  id: i.id.replace(/\d+$/, String(9001 + n)),
  attention: { state: "hidden" as const, rule: 2, reason: "Hidden by rule 2: Hide Acme Deals" },
}));

/** One action that rule 5 approved and Iris applied, for the tests of the handled view (apply mode). */
export const decided: HandledPage = {
  count: 1,
  next: null,
  actions: [
    {
      changeset: "cs-14",
      revision: 1,
      n: 1,
      verb: "archive",
      args: {},
      messages: 500,
      state: "applied",
      rule: 5,
      policy: 3,
      why: "Feed items that you did not open in 30 days.",
      reason: "Rule 5 (Archive old newsletters) allows archive.",
      at: "2026-11-16T08:12:00Z",
    },
  ],
};

/** GET /api/handled in the fixture state: observe mode, so no rule decided an action. */
export const handled: HandledPage = { count: 0, next: null, actions: [] };

// The history view (card R2-07): ledger entries of each kind, newest first.
const entry = (id: number, at: string, rest: Partial<LedgerEntry>): LedgerEntry => ({
  id: `l-${id}`,
  verb: "archive",
  args: {},
  state: "applied",
  at,
  error: null,
  messages: 0,
  changed: 0,
  items: 0,
  title: null,
  changeset: `cs-${id + 100}`,
  author: { kind: "person", name: null, job: null, goal: null },
  decided_by: "person",
  rule: null,
  policy: null,
  reason: null,
  why: null,
  can_undo: false,
  undo_open: false,
  undone_by: null,
  undo_of: null,
  ...rest,
});
export const ledger: LedgerPage = {
  next: null,
  entries: [
    entry(31, "2026-11-17T09:42:00Z", { messages: 18, changed: 18, author: { kind: "triage", name: null, job: null, goal: null }, decided_by: "rule", rule: 5, policy: 3, reason: "Rule 5 (Archive old newsletters) allows archive.", why: "Newsletters that you did not open in a week.", can_undo: true }),
    entry(30, "2026-11-17T09:40:00Z", { verb: "trash", messages: 2, changed: 2, why: "Sales mail from senders that you do not know.", can_undo: true }),
    entry(29, "2026-11-17T09:15:00Z", { verb: "label", args: { label: "Finance" }, messages: 3, changed: 3, author: { kind: "agent", name: "Claude Code", job: "j-4", goal: "Pay the Acme invoice" }, why: "The invoices of the Acme order.", can_undo: true }),
    entry(28, "2026-11-17T08:30:00Z", { verb: "unread", messages: 6, changed: 6, why: "Undo of l-12", undo_of: "l-12", can_undo: true }),
    entry(27, "2026-11-16T18:20:00Z", { state: "failed", messages: 40, changed: 12, error: "Gmail refused one call.", why: "Old notices from Example Air.", can_undo: true }),
    entry(26, "2026-11-16T17:05:00Z", { verb: "close", items: 1, title: "Send the Q4 numbers to Priya", why: "Done." }),
    entry(25, "2026-11-16T08:02:00Z", { verb: "read", state: "undone", messages: 6, changed: 6, author: { kind: "triage", name: null, job: null, goal: null }, decided_by: "rule", rule: 5, policy: 3, why: "Receipts that need no reply.", undone_by: "l-32" }),
  ],
};

/** POST /api/ledger/{id}/undo: the entry with its open undo. */
export function undoEntry(id: string): LedgerEntry | undefined {
  const e = ledger.entries.find((x) => x.id === id && x.can_undo);
  if (!e) return undefined;
  Object.assign(e, { can_undo: false, undo_open: true });
  return e;
}

export function message(id: string): Message | undefined {
  const item = items.find((i) => i.sources.some((s) => s.message === id));
  if (!item) return askMail(id);
  const source = item.sources.find((s) => s.message === id)!;
  return {
    id,
    thread: source.thread,
    date: item.created,
    from: { name: "Example Sender", address: "sender@example.com" },
    subject: item.title,
    labels: ["INBOX"],
    // A mail with a fixed head has the head of its summary everywhere.
    ...(HEADS[id] ? (summaryOf(id) as Summary) : {}),
    to: "you@example.com",
    cc: null,
    text: ASK_MAIL[id] ?? `${source.quote ?? item.title}\n\nThis is a fixture message, made from the open model examples.`,
    gone: false,
    hostile: [],
    items: items.filter((i) => i.sources.some((s) => s.message === id)).map((i) => i.id),
  };
}

/** The original source of a fixture message (RFC 5322), for /api/messages/<id>/raw. */
export function raw(id: string): string | undefined {
  const m = message(id);
  if (!m) return undefined;
  return [
    `From: ${m.from.name} <${m.from.address}>`,
    `To: ${m.to ?? ""}`,
    `Subject: ${m.subject}`,
    `Date: ${new Date(m.date ?? 0).toUTCString()}`,
    `Message-ID: <${id}@fixtures.example>`,
    "Content-Type: text/plain; charset=utf-8",
    "",
    m.text,
    "",
  ].join("\r\n");
}

const rule = (r: Partial<Rule> & Pick<Rule, "id" | "name" | "effect" | "when">): Rule => ({ verbs: [], by: "person", added: "2026-11-10", job: null, line: 0, ...r });

/** The rules of spec 2 section 6.1, with one change of the file to confirm. */
const rules: Rule[] = [
  rule({ id: 1, name: "Archive newsletters after 3 days", effect: "allow", verbs: ["archive", "read"], when: { form: "feed", sender_kind: "service", older_than_days: 3 }, line: 10 }),
  rule({ id: 2, name: "Hide Acme Deals", effect: "hide", when: { sender: "deals@acme.example" }, by: "proposal", added: "2026-11-12", line: 19 }),
  rule({ id: 3, name: "Bundle the feed", effect: "bundle", when: { form: "feed" }, line: 27 }),
  rule({
    id: 4,
    name: "Never touch mail from the bank without review",
    effect: "review",
    verbs: ["archive", "inbox", "label", "unlabel", "read", "unread", "trash"],
    when: { sender: "@bank.example" },
    line: 35,
  }),
  // Notify rules (R5-08): the rules of the notices below.
  rule({ id: 7, name: "Mail from the clinic", effect: "notify", when: { sender: "@clinic.example" }, added: "2026-11-16", line: 44 }),
  rule({ id: 8, name: "Replies from Dana", effect: "notify", when: { sender: "dana@harbor.example" }, added: "2026-11-16", line: 51 }),
  rule({ id: 9, name: "Bank security codes", effect: "notify", when: { sender: "@bank.example", form: "signal" }, added: "2026-11-16", line: 58 }),
  // A grant inside job j-12 (card R4-10): the verbs of "Tidy the mail".
  rule({ id: 11, name: "In job j-12, allow archive, label, unlabel, read, unread and inbox without review", effect: "allow", verbs: ["archive", "label", "unlabel", "read", "unread", "inbox"], when: { author: "agent" }, job: "j-12", added: "2026-11-16", line: 65 }),
];

export const policy: Policy = {
  mode: "observe",
  version: 3,
  next_id: 12,
  rules,
  proposals: [
    {
      id: 1,
      kind: "approvals",
      approvals: 14,
      created_at: 1794787200,
      rule: rule({ id: 10, name: "Archive mail from deals@acme.example", effect: "allow", verbs: ["archive"], when: { sender: "deals@acme.example" }, by: "proposal", added: "2026-11-16", line: 0 }),
    },
    {
      id: 2,
      approvals: 11,
      created_at: 1794790800,
      rule: rule({ id: 10, name: "Mark mail from services as read", effect: "allow", verbs: ["read"], when: { sender_kind: "service" }, by: "proposal", added: "2026-11-16", line: 0 }),
    },
    {
      // A hide rule from the skips of the person (R5-02).
      id: 3,
      kind: "choices",
      approvals: 12,
      created_at: 1794794400,
      rule: rule({ id: 10, name: "Hide feed items from digest@news.example", effect: "hide", when: { sender: "digest@news.example", form: "feed" }, by: "proposal", added: "2026-11-16", line: 0 }),
    },
  ],
  error: null,
  file: {
    state: "changed",
    sha256: "0".repeat(64),
    text: "version = 0\nmode = \"apply\"\n# The rest of the file.\n",
    mode: "apply",
    added: [rule({ id: 5, name: "Mark receipts as read", effect: "allow", verbs: ["read"], when: { form: "record", record_type: "receipt" }, added: "2026-11-16", line: 44 })],
    removed: [],
    changed: [{ before: rules[0], after: { ...rules[0], name: "Archive newsletters after 5 days", when: { ...rules[0].when, older_than_days: 5 } } }],
  },
};

const HEADS: Record<string, [string, string, string, string]> = {
  "m-50101": ["t-7712", "Morning Brew", "crew@morningbrew.example", "The week in markets, in five minutes"],
  "m-50102": ["t-7712", "Morning Brew", "crew@morningbrew.example", "Why oil fell on Tuesday"],
  "m-50140": ["t-7713", "Product Hunt Daily", "hello@producthunt.example", "Top launches of the week"],
  "m-55210": ["t-8841", "Priya Raman", "priya@northwind.example", "Q4 numbers for the board"],
  "m-53500": ["t-8600", "Example Air", "trips@air.example", "Your flight to Tokyo: QX7K2P"],
  "m-55211": ["t-8841", "You", "you@example.com", "Re: Q4 numbers for the board"],
  "m-55212": ["t-8841", "Priya Raman", "priya@northwind.example", "Re: Q4 numbers for the board"],
  "m-55380": ["t-9001", "Sam Lee", "sam@example.org", "Dinner on Thursday?"],
  "m-55390": ["t-9001", "You", "you@example.com", "Re: Dinner on Thursday?"],
  "m-55400": ["t-9001", "Sam Lee", "sam@example.org", "Re: Dinner on Thursday?"],
  "m-60001": ["t-9002", "Dana Brooks", "dana@leadgen.example", "Quick question about your Q1 plans"],
  "m-9001": ["t-9001", "Dana Wu", "dana@acme.example", "Renewal for 2027"],
  "m-9002": ["t-9001", "Dana Wu", "dana@acme.example", "Re: Renewal for 2027"],
  "m-9003": ["t-9050", "Acme Billing", "billing@acme-pay.example", "Change of bank details"],
  "m-61500": ["t-9100", "Morning Brew", "crew@morningbrew.example", "Monday: the market opens higher"],
};
const PUBLISHERS = [
  ["Morning Brew", "crew@morningbrew.example", "Your morning in markets"],
  ["Stratechery", "email@stratechery.example", "The weekly article"],
  ["The Verge", "newsletter@theverge.example", "Verge Deals of the day"],
  ["Product Hunt Daily", "hello@producthunt.example", "Top launches of the day"],
] as const;

/** The text of each mail that a fixture answer of Ask cites. */
const ASK_MAIL: Record<string, string> = {
  "m-55210": "Hi,\n\nCould you review section 4 by Tuesday? The board meets on Wednesday.\n\nPriya",
  "m-55211": "Thanks, Priya. I will send it on Monday.",
  "m-53500": "Booking reference QX7K2P\n\nYour flight to Tokyo leaves on 10 December at 11:05 and lands on 11 December at 07:40, Tokyo time. The gate shows 24 hours before departure.",
};

/** A mail that Ask cites, with the head of its summary, so its link opens it. */
function askMail(id: string): Message | undefined {
  const text = ASK_MAIL[id];
  if (text === undefined) return undefined;
  const s = summaryOf(id) as Summary;
  return { ...s, to: "you@example.com", cc: null, text, gone: false, hostile: [], items: [] };
}

/** POST /api/ask: a fixed answer for each kind of question, with no model. */
export function askAnswer(question: string): AskAnswer {
  const source = (n: number, id: string, quote: string | null) => ({ n, quote, message: summaryOf(id) as Summary });
  if (/weather|lottery/i.test(question)) return { answer: null, sources: [], read: 0, cost_micro_usd: null };
  if (/flight|tokyo|trip/i.test(question)) {
    return {
      answer: "Your flight to Tokyo leaves on 10 December at 11:05 [1]. The booking reference is QX7K2P [1].",
      sources: [source(1, "m-53500", "Booking reference QX7K2P")],
      read: 6,
      cost_micro_usd: 3_120,
    };
  }
  return {
    answer: "Priya asked you to review section 4 of the Q4 numbers by Tuesday [1]. You answered that you will send it on Monday [2].",
    sources: [source(1, "m-55210", "Could you review section 4 by Tuesday?"), source(2, "m-55211", null)],
    read: 9,
    cost_micro_usd: 4_180,
  };
}

/** A summary for each fixture message ID. The triage IDs get a newsletter head. */
function summaryOf(id: string): Target {
  const num = Number(id.slice(2));
  const head = HEADS[id];
  if (id === "m-50140") return { id, gone: true };
  const [thread, name, address, subject] = head ?? [`t-${num}`, ...PUBLISHERS[num % PUBLISHERS.length]];
  const date = new Date(Date.UTC(2026, 10, 15, 8) - (num % 61) * 3_600_000).toISOString();
  return { id, thread, date, from: { name, address }, subject, labels: ["INBOX"] };
}

const action = (a: Partial<Action> & Pick<Action, "n" | "verb" | "resolved">): Action => ({
  revision: 1,
  targets: { messages: a.resolved.messages },
  args: {},
  why: null,
  flags: [],
  hash: `${a.n}`.padStart(64, "0"),
  decision: { state: "in_review", by: "policy", reason: "No rule allows this change." },
  ...a,
});
const range = (from: number, count: number) => Array.from({ length: count }, (_, i) => `m-${from + i}`);

/** Two open changesets: a request of an agent, and the triage of the newsletters (spec 2 section 5.1). */
export const changesets: Changeset[] = [
  {
    id: "cs-77",
    revision: 1,
    author: { kind: "agent", name: "Claude Code", session: "se-12", job: "j-142" },
    why: "Priya needs the Q4 numbers by Friday. I cleared the noise around her thread, so you see it first.",
    created: "2026-11-16T09:17:02Z",
    submitted: "2026-11-16T09:17:40Z",
    state: "open",
    actions: [
      action({ n: 1, verb: "label", targets: { threads: ["t-8841"] }, resolved: { messages: ["m-55210", "m-55211"] }, args: { label: "Finance" }, why: "The Q4 thread, so you find it fast." }),
      action({ n: 2, verb: "archive", targets: { threads: ["t-7712", "t-7713"] }, resolved: { messages: ["m-50101", "m-50102", "m-50140"] }, why: "Two newsletters from last week." }),
      action({ n: 3, verb: "read", resolved: { messages: ["m-60001"] }, why: "A cold sales mail. You do not answer these.", flags: ["first_contact"] }),
      action({
        n: 4,
        verb: "archive",
        resolved: { messages: ["m-61500"] },
        why: "A newsletter from Monday.",
        decision: { state: "approved", by: "rule", rule: 1, policy: 3, at: "2026-11-16T09:17:41Z" },
      }),
    ],
  },
  {
    id: "cs-78",
    revision: 1,
    author: { kind: "triage" },
    why: "Newsletters older than 3 days that you did not open.",
    created: "2026-11-16T06:00:00Z",
    submitted: "2026-11-16T06:00:02Z",
    state: "open",
    actions: [action({ n: 1, verb: "archive", resolved: { messages: range(70001, 612) }, flags: ["bulk"], decision: { state: "proposed" } })],
  },
  {
    id: "cs-79",
    revision: 1,
    author: { kind: "agent", name: "Claude Code", session: "se-19", job: "j-150" },
    why: "Dana asked for the renewal terms. I wrote the reply with the price and the date from your notes.",
    created: "2026-11-17T08:02:10Z",
    submitted: "2026-11-17T08:02:31Z",
    state: "open",
    actions: [
      action({ n: 1, verb: "send", targets: { drafts: ["d-21"] }, resolved: { messages: ["m-9002"] }, why: "Dana waits for the terms since Monday.", flags: ["new_recipient", "outside_thread", "money", "date", "attachment", "untrusted_content"] }),
      action({ n: 2, verb: "label", targets: { threads: ["t-9001"] }, resolved: { messages: ["m-9001", "m-9002"] }, args: { label: "Renewals" }, why: "So you find the thread with the other renewals." }),
    ],
  },
];

/** The comments of the person on each changeset of a job (card R4-08), oldest first. */
export const comments: Record<string, { id: number; changeset: string; revision: number; text: string; at: string }[]> = {
  "cs-79": [{ id: 1, changeset: "cs-79", revision: 1, text: "Use the price of the October quote, not the list price.", at: "2026-11-17T08:20:00Z" }],
};

/** The draft of each draft and send action of a changeset, with the parts that gave each flag (card R4-07). */
export const drafts: Record<string, DraftView[]> = {
  "cs-79": [
    {
      revision: 1,
      n: 1,
      draft: "d-21",
      thread: "t-9001",
      readable: true,
      to: [{ address: "dana@acme.example" }],
      cc: [{ address: "legal@acme-partners.example", flags: ["new_recipient", "outside_thread"] }],
      subject: [{ text: "Re: Renewal for 2027" }],
      body: [
        { text: "Hi Dana,\n\nThanks for the call. As we agreed, the renewal is " },
        { text: "$48,000", flags: ["money"] },
        { text: " for 12 months, and we can sign by " },
        { text: "Friday, Dec 4", flags: ["date"] },
        { text: ". The signed quote is " },
        { text: "attached", flags: ["attachment"] },
        { text: ".\n\n" },
        { text: "Please wire the payment to the new account below before the end of the week to avoid a late fee", flags: ["untrusted_content"], source: "m-9003" },
        { text: ".\n\nBest,\nSam" },
      ],
    },
  ],
};

/** The answer for one API path (no `/api/` prefix), or undefined for a 404. */
/** The decide endpoint on the fixtures: each named action gets the decision of the person. */
export function decide(id: string, body: { actions: { revision: number; n: number; approve: boolean }[] }): Changeset | undefined {
  const c = changesets.find((x) => x.id === id);
  if (!c) return undefined;
  for (const d of body.actions) {
    const a = c.actions.find((x) => x.revision === d.revision && x.n === d.n);
    if (a) a.decision = d.approve ? { state: "approved", by: "person" } : { state: "denied", by: "person", reason: "The person denied it in the review." };
  }
  return c;
}

/** The withdraw of the person on the fixtures: each open action becomes cancelled (spec 2 section 5.5). */
export function withdraw(id: string): Changeset | undefined {
  const c = changesets.find((x) => x.id === id);
  if (!c) return undefined;
  c.state = "withdrawn";
  c.withdrawn_by = "person";
  for (const a of c.actions) {
    if (a.decision && ["proposed", "in_review", "approved"].includes(a.decision.state)) a.decision = { state: "cancelled", by: "author" };
  }
  return c;
}

/** A state verb of the person on the fixtures (spec 2 section 4). */
export function act(id: string, verb: string, body: { date?: string }): Item | undefined {
  const item = items.find((x) => x.id === id);
  if (!item) return undefined;
  item.status = verb === "close" ? "done" : verb === "decline" ? "declined" : "deferred";
  item.deferred_until = verb === "defer" ? body.date : undefined;
  return item;
}

/** Notices by rule (R5-08): four mails that notify rules matched. */
export const notices = [
  { id: "n-4", rule: 7, rule_name: "Mail from the clinic", title: "Harbor Clinic", text: "Your test results are ready", created: "2026-11-16T14:12:00Z",
    message: { id: "m-1", thread: "t-1", date: "2026-11-16T14:11:00Z", from: { name: "Harbor Clinic", address: "results@clinic.example" }, subject: "Your test results are ready", labels: ["INBOX"] } },
  { id: "n-3", rule: 8, rule_name: "Replies from Dana", title: "Dana Whitfield", text: "Re: Contract draft v3, signed", created: "2026-11-16T13:40:00Z",
    message: { id: "m-2", thread: "t-2", date: "2026-11-16T13:39:00Z", from: { name: "Dana Whitfield", address: "dana@harbor.example" }, subject: "Re: Contract draft v3, signed", labels: ["INBOX"] } },
  { id: "n-2", rule: 7, rule_name: "Mail from the clinic", title: "Harbor Clinic", text: "Appointment moved to Thursday", created: "2026-11-16T11:05:00Z", message: null },
  { id: "n-1", rule: 9, rule_name: "Bank security codes", title: "First Harbor Bank", text: "Your sign-in code", created: "2026-11-16T09:00:00Z",
    message: { id: "m-3", thread: "t-3", date: "2026-11-16T09:00:00Z", from: { name: "First Harbor Bank", address: "alerts@bank.example" }, subject: "Your sign-in code", labels: ["INBOX"] } },
];

export function noticePost(path: string): [number, unknown] | undefined {
  const done = path.match(/^\/notices\/([^/?]+)\/done$/);
  if (!done) return undefined;
  const i = notices.findIndex((n) => n.id === decodeURIComponent(done[1]));
  if (i < 0) return [404, { error: "not found" }];
  notices.splice(i, 1);
  return [200, { done: done[1] }];
}

/** Watches (R5-07): one mail that came, and two watches that wait. */
export const watches = [
  {
    id: "w-3",
    text: "visa from:consulate",
    created: "2026-11-09T08:00:00Z",
    state: "arrived",
    arrived: "2026-11-16T09:42:00Z",
    message: { id: "m-90", thread: "t-90", date: "2026-11-16T09:41:00Z", from: { name: "Consulate Visa Office", address: "visa@consulate.example" }, subject: "Your visa application: decision", labels: ["INBOX"] },
  },
  { id: "w-2", text: "\"lease renewal\" from:harbor", created: "2026-11-12T10:00:00Z", state: "waits", arrived: null, message: null },
  { id: "w-1", text: "invoice from:dana", created: "2026-11-02T10:00:00Z", state: "waits", arrived: null, message: null },
];

let lastWatch = 10;

export function watchPost(path: string, body: { text?: string }): [number, unknown] {
  const remove = path.match(/^\/watches\/([^/?]+)\/remove$/);
  if (remove) {
    const i = watches.findIndex((w) => w.id === decodeURIComponent(remove[1]));
    if (i < 0) return [404, { error: "not found" }];
    watches.splice(i, 1);
    return [200, { removed: remove[1] }];
  }
  const text = (body.text ?? "").trim();
  if (!text) return [400, { error: "watch: the watch has no search text" }];
  const w = { id: `w-${++lastWatch}`, text, created: new Date().toISOString(), state: "waits", arrived: null, message: null };
  watches.unshift(w);
  return [201, w];
}

// Jobs of the agents (card R4-09): one in each state.
const at = (day: number, h: number, m = 0) => new Date(Date.UTC(2026, 10, day, h, m)).toISOString();
const dana = { name: "Dana Whitfield", address: "dana@northwind.example" };
export const jobs: Record<string, Job> = {
  "j-12": { id: "j-12", goal: "Get the signed contract from Dana before 20 Nov.", due: at(20, 0), created: at(16, 9), updated: at(16, 18, 41), state: "waiting", wait: { thread: "t-8841", until: at(18, 9) } },
  "j-14": {
    id: "j-14",
    goal: "Pay the Q4 invoice from Brightline.",
    due: at(17, 17),
    created: at(16, 8),
    updated: at(16, 10, 2),
    state: "needs_you",
    question: "The invoice says 2,400 EUR, but the quote said 2,150 EUR. Pay the full amount, or ask Brightline about the difference first?",
  },
  "j-15": { id: "j-15", goal: "Book a dentist appointment for next week, in the morning.", created: at(16, 11), updated: at(16, 11), state: "working" },
  "j-9": { id: "j-9", goal: "Cancel the gym membership.", created: at(12, 9), updated: at(13, 15), state: "done", end: "complete" },
};
export const jobDetails: Record<string, JobDetail> = {
  "j-12": {
    job: {
      ...jobs["j-12"],
      events: [
        { at: at(16, 9), state: "working", by: "person" },
        { at: at(16, 9, 20), state: "waiting", by: "agent", wait: { thread: "t-8841", until: at(17, 9) }, why: "I asked Dana for the signed copy. She said she sends it today." },
        { at: at(16, 16, 5), state: "working", by: "engine", why: "reply" },
        { at: at(16, 16, 12), state: "needs_you", by: "agent", question: "Dana sent a version with a new clause 7 (payment in 45 days, not 30). Accept it?" },
        { at: at(16, 18, 30), state: "working", by: "person", answer: "Accept 45 days. Ask her to sign today." },
        { at: at(16, 18, 41), state: "waiting", by: "agent", wait: { thread: "t-8841", until: at(18, 9) }, why: "Dana signs tomorrow morning." },
      ],
    },
    changesets: [
      { id: "cs-31", at: at(16, 9, 18), state: "closed", why: "File the old drafts of the contract, so only the newest stays in the inbox.", actions: { applied: 3 } },
      { id: "cs-34", at: at(16, 18, 38), state: "open", why: "Reply to Dana: accept clause 7 and ask for the signature.", actions: { in_review: 1 } },
    ],
    threads: { "t-8841": { id: "m-90211", thread: "t-8841", date: at(16, 16, 2), from: dana, subject: "Re: Northwind contract, final version", labels: ["INBOX"], gone: false } },
  },
  "j-14": {
    job: { ...jobs["j-14"], events: [{ at: at(16, 8), state: "working", by: "person" }, { at: at(16, 10, 2), state: "needs_you", by: "agent", question: jobs["j-14"].question, why: "The amounts do not agree." }] },
    changesets: [],
    threads: {},
  },
  // A working job whose session ended with an error: no agent works on it (issue #742).
  "j-15": {
    job: { ...jobs["j-15"], events: [{ at: at(16, 11), state: "working", by: "person" }] },
    changesets: [],
    threads: {},
    session: { id: "se-3", agent: "claude-code", state: "ended", end: "exit", exit: 1, ended: at(16, 11, 1) },
  },
};
const jobPage = (state: string | null) => {
  const list = Object.values(jobs)
    .filter((j) => (state && state !== "open" ? j.state === state : j.state !== "done"))
    .sort((a, b) => Number(b.id.slice(2)) - Number(a.id.slice(2)));
  return { jobs: list, next: null };
};

/** The stop switch of the fixtures (docs/specs/2026-11-17-stop.md). */
let stoppedAt: string | null = null;

/** POST /api/stop and /api/resume, as the API answers. */
export function stopPost(path: string): [number, unknown] | undefined {
  if (path === "/stop") {
    stoppedAt ??= new Date().toISOString();
    return [200, { stopped: true, since: stoppedAt, sessions: 1, live: 0, busy: false }];
  }
  if (path === "/resume") {
    const resumed = stoppedAt !== null;
    stoppedAt = null;
    return [200, { stopped: false, since: null, resumed }];
  }
  return undefined;
}

export function answer(path: string): unknown {
  const url = new URL(path, "http://local/");
  const q = url.searchParams;
  const [name, id] = url.pathname.slice(1).split("/");
  if (name === "status") return status;
  if (name === "stop" && !id) return { stopped: stoppedAt !== null, since: stoppedAt, live: 0, busy: false };
  if (name === "brief" && id?.startsWith("bundle-")) {
    const bundle = brief.bundles?.find((b) => `bundle-${b.rule}` === id);
    return bundle ? { count: bundle.count, items: bundle.items, next: null } : undefined;
  }
  if (name === "records") return { receipt: 1, booking: 1 };
  if (name === "brief" && id === "hidden") return { count: hidden.length, items: hidden, next: null };
  if (name === "handled") return handled;
  if (name === "ledger" && !id) return ledger;
  if (name === "outbox") return outbox();
  if (name === "gate") return page(id === "declined" ? "declined" : "waiting");
  if (name === "brief" && id) {
    const section = briefNow()[id as "you_owe"];
    return section && "items" in section ? { items: section.items, next: null, duplicates: section.duplicates } : undefined;
  }
  if (name === "brief") return briefNow();
  if (name === "items" && id && url.pathname.endsWith("/job")) return startPreview(id);
  if (name === "items" && id) return [...items, ...hidden, ...strangers].find((i) => i.id === id);
  if (name === "items") {
    return { items: items.filter((i) => (!q.get("form") || i.form === q.get("form")) && (!q.get("type") || i.type === q.get("type"))), next: null };
  }
  if (name === "messages" && id) return message(id);
  if (name === "messages" && q.get("ids")) return { messages: q.get("ids")!.split(",").map(summaryOf) };
  if (name === "changesets" && id && url.pathname.endsWith("/comments")) return changesets.some((c) => c.id === id) ? { comments: comments[id] ?? [] } : undefined;
  if (name === "changesets" && id && url.pathname.endsWith("/drafts")) return drafts[id] ? { drafts: drafts[id] } : undefined;
  if (name === "changesets" && id) return changesets.find((c) => c.id === id);
  if (name === "changesets") return { changesets };
  if (name === "search") return { messages: [] };
  if (name === "policy") return policy;
  if (name === "watches") return watches;
  if (name === "matters" && id) return matterViews[id];
  if (name === "matters") return matters;
  if (name === "jobs" && id) return jobDetails[id];
  if (name === "jobs") return jobPage(q.get("state"));
  if (name === "notices") return notices;
  if (name === "measures") return measures;
  if (name === "threads" && id === "t-8841" && url.pathname.endsWith("/reply")) return replyStart;
  if (name === "threads" && id === "t-8700" && url.pathname.endsWith("/draft")) return nudgeDraft;
  return undefined;
}

/** GET /api/threads/t-8841/reply: the start values of a reply, with the conversation (R3-04). */
const replyStart: ReplyStart = {
  account: "a-1",
  parent: { id: "m-55212", from: { name: "Priya Raman", address: "priya@northwind.example" }, date: "2026-11-17T09:12:00Z", text: "Thanks. Could you also add the Q3 numbers next to them?" },
  to: ["priya@northwind.example"],
  cc: [],
  subject: "Re: Q4 numbers for the board",
  topic: "Re: Q4 numbers for the board",
  conversation: [
    { id: "m-55210", from: { name: "Priya Raman", address: "priya@northwind.example" }, date: "2026-11-14T16:40:00Z", to: "you@example.com", you: false, hostile: false, text: ASK_MAIL["m-55210"], cut: false },
    { id: "m-55211", from: { name: null, address: "you@example.com" }, date: "2026-11-15T08:05:00Z", to: "priya@northwind.example", you: true, hostile: false, text: ASK_MAIL["m-55211"], cut: false },
    { id: "m-55212", from: { name: "Priya Raman", address: "priya@northwind.example" }, date: "2026-11-17T09:12:00Z", to: "you@example.com", you: false, hostile: false, text: "Thanks. Could you also add the Q3 numbers next to them, so the board sees the trend?\n\nPriya", cut: false },
  ],
  earlier: 0,
};

// Matters: the two examples, with members from the item examples above.
const members = (base: object, list: Item[]): MatterView => {
  const by = (form: string) => list.filter((i) => i.form === form);
  const items = { commitment: by("commitment"), conversation: by("conversation"), record: by("record") };
  const counts = { commitment: items.commitment.length, conversation: items.conversation.length, record: items.record.length };
  return { matter: { ...(base as Matter), items: list.map((i) => i.id), counts }, items };
};
export const matterViews: Record<string, MatterView> = {
  "g-4": members(matterConfirmed, [youOwe, conversation, booking] as unknown as Item[]),
  "g-5": members(matterProposed, [receipt, owedToYou] as unknown as Item[]),
};
export const matters: MatterList = { proposed: [matterViews["g-5"].matter], confirmed: [matterViews["g-4"].matter] };

/** POST /api/senders/<id>/<answer> in the fixture state: the sender row after the answer. */
// The answers at the gate in the dev server: the state of each sender, from the fixtures.
const senders = [...gate.senders, ...ignored.senders];
const state = new Map<string, "waiting" | "declined" | "accepted">(senders.map((s) => [s.id, gate.senders.includes(s) ? "waiting" : "declined"]));
const page = (want: string): GatePage => {
  const list = senders.filter((s) => state.get(s.id) === want);
  return { count: list.length, senders: list, next: null };
};
/** The brief after the answers: an accepted sender's items go to their sections. */
function briefNow(): Brief {
  const moved = senders.filter((s) => state.get(s.id) === "accepted").flatMap((s) => s.items);
  const waiting = senders.filter((s) => state.get(s.id) === "waiting").flatMap((s) => s.items);
  const add = (section: Brief["you_owe"], more: Item[]) => ({ ...section, ...of([...section.items, ...more.filter((i) => !section.items.includes(i))]) });
  return {
    ...brief,
    you_owe: add(brief.you_owe, moved.filter((i) => i.form === "commitment")),
    conversations: add(brief.conversations, moved.filter((i) => i.form === "conversation")),
    strangers: of(strangers.filter((i) => waiting.includes(i))),
    gate: page("waiting"),
    declined: page("declined").count,
  };
}

/** Accept, decline or reopen a sender of the gate, as the engine does. */
export function answerSender(id: string, answer: string): GateAnswer | undefined {
  const sender = senders.find((s) => s.id === id);
  if (!sender || !["accept", "decline", "reopen"].includes(answer)) return undefined;
  state.set(id, answer === "accept" ? "accepted" : answer === "decline" ? "declined" : "waiting");
  const accepted = answer === "accept";
  return { id, address: sender.address, name: sender.name, kind: accepted ? "person" : "stranger", decided_by: accepted ? "person" : "engine", declined: answer === "decline" };
}

// "Handle this" (card R4-10): the mail that a start grants. Message m-55212 is not readable yet.
const START: Record<string, [string, boolean][]> = {
  "c-311": [["m-55210", true], ["m-55211", true], ["m-55212", false]],
  "v-88": [["m-55380", true], ["m-55390", true], ["m-55400", true]],
};
function startPreview(id: string) {
  const list = START[id];
  // The engine order: the thread, then the date.
  return list && { messages: list.map(([m, readable], i) => ({ ...(summaryOf(m) as Summary), date: at(15, 9 + 3 * i, 12), readable })) };
}
/** POST /api/jobs/<id>/answer, as the engine answers: the job with the answer, 400 for no text, 409 with no question (R5-05). */
export function answerJob(id: string, body: { answer?: unknown; question?: unknown; updated?: unknown }): [number, unknown] {
  const job = jobs[id];
  if (!job) return [404, { error: "not found" }];
  const said = body.answer;
  if (typeof said !== "string" || !said.trim()) return [400, { error: "the answer must have 1 to 10000 characters and no NUL character" }];
  if (typeof body.question !== "string" || typeof body.updated !== "string") return [400, { error: "the body needs question and updated" }];
  if (job.state !== "needs_you" || job.question !== body.question || job.updated !== body.updated) return [409, { error: "The job does not ask this question now." }];
  const asked = jobDetails[id]?.job.events ?? [];
  jobs[id] = { ...job, state: "working", question: undefined, updated: new Date().toISOString() };
  const events = [...asked, { at: jobs[id].updated, state: "working" as const, by: "person" as const, answer: said }];
  // The engine starts a new session with the answer.
  const session = { id: "se-4", agent: "claude-code", state: "starting" as const, end: null, exit: null, ended: null };
  jobDetails[id] = { job: { ...jobs[id], events }, changesets: jobDetails[id]?.changesets ?? [], threads: jobDetails[id]?.threads ?? {}, session };
  return [200, jobDetails[id]];
}
/** POST /api/jobs: the fixture job j-12 stands for the new job. */
export function startJob(): [number, unknown] {
  return [201, jobs["j-12"]];
}
/** POST /api/policy/grant and /api/policy/remove on the fixture policy. */
export function policyPost(path: string, body: { job?: string; verbs?: string[]; id?: number }): [number, unknown] {
  if (path === "/policy/grant" && body.job && body.verbs?.length) {
    const r = rule({ id: policy.next_id++, name: `In job ${body.job}, allow ${body.verbs.join(", ")} without review`, effect: "allow", verbs: body.verbs, when: { author: "agent" }, job: body.job, added: "2026-11-17", line: 70 });
    policy.rules = [...policy.rules, r];
  } else if (path === "/policy/remove" && body.id) {
    policy.rules = policy.rules.filter((r) => r.id !== body.id);
  } else return [404, { error: "Not found." }];
  policy.version = (policy.version ?? 0) + 1;
  return [200, policy];
}

/** The outbox of the fixture state: the holds run from the load of the dev server. */
const outboxStart = Date.now();
const outboxCancelled = new Set<string>();

/** GET /api/outbox: sending is on, and three sends wait. A cancel removes a send. */
export function outbox(): Outbox {
  const at = (s: number) => new Date(outboxStart + s * 1000).toISOString();
  const sends: Outbox["sends"] = [
    { draft: "d-14", state: "held", approved: at(-18), leaves: at(42), flags: [], readable: true, to: ["dana@acme.example"], cc: [], subject: "Re: Signing on Friday" },
    { draft: "d-15", state: "held", approved: at(-5), leaves: at(55), flags: ["new_recipient", "money"], readable: true, to: ["dana@acme.example"], cc: ["legal@new.example"], subject: "Renewal: $48,000 for 12 months" },
    { draft: "d-16", state: "approved", approved: at(0), leaves: at(60), flags: ["attachment"], readable: true, to: ["priya@example.com"], cc: [], subject: "The quote" },
  ].filter((x) => !outboxCancelled.has(x.draft)) as Outbox["sends"];
  return { sending: { on: true, since: at(-3600) }, count: sends.length, next: null, sends };
}

/** POST /api/outbox/<d-N>/cancel, as the API answers: 200, or 404 for a draft with no send that waits. */
export function outboxPost(path: string): [number, unknown] | undefined {
  const cancel = path.match(/^\/outbox\/([^/?]+)\/cancel$/);
  if (!cancel) return undefined;
  const draft = decodeURIComponent(cancel[1]);
  if (!outbox().sends.some((x) => x.draft === draft)) return [404, { error: "Not found." }];
  outboxCancelled.add(draft);
  return [200, { cancelled: draft }];
}

/** GET /api/measures in the fixture state: two weeks of use, with two days off. */
export const measures: Measures = {
  days: [
    ["2026-11-16", 2], ["2026-11-15", 0], ["2026-11-14", 1], ["2026-11-13", 2], ["2026-11-11", 1], ["2026-11-10", 3],
    ["2026-11-09", 0], ["2026-11-08", 2], ["2026-11-07", 1], ["2026-11-05", 3], ["2026-11-04", 2], ["2026-11-03", 4],
  ].map(([day, opens]) => ({ day: day as string, opens: opens as number })),
  first_brief: { connect: "2026-11-03T08:00:00Z", brief: "2026-11-03T08:05:40Z", seconds: 340 },
  mirror: [{ account: "owner@example.com", checked: "2026-11-16T07:12:00Z", differences: 0, complete: true, skipped: 0, stopped: false }],
  memory: { rss_kib: 31_744, messages: 18_402 },
  approvals: { decided: 50, approved: 47 },
  undo: { applied: 48, undone: 1 },
};

/** GET /api/threads/t-8700/draft: the nudge to Alex, who is late with the SDK examples (R3-07). */
export const nudgeDraft = {
  thread: "t-8700",
  based_on: "m-54001",
  nudge: true,
  to: "alex@example.com",
  item: "c-207",
  body: "Hi Alex,\n\nA quick check on the SDK examples. When can you send them?\n\nThanks",
};

/** POST /api/threads/t-8841/draft: a draft of the reply in your voice (R3-04 intent path). */
export const replyDraft = {
  body: "Hi Priya,\n\nYes, I will add the Q3 numbers next to Q4, so the board sees the trend. You get the deck on Monday morning.\n\nThanks",
  based_on: "m-55212",
  samples: 6,
  unknown_cost: false,
  overrun: false,
};
