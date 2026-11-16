// Answers of the local API, made from the examples of docs/spec/schema/examples.
// The dev server (`pnpm dev --mode fixtures`) and the tests use them. They are not in the build.
import type { Action, Brief, Changeset, HandledPage, Item, Message, Policy, Rule, Status, Target } from "../src/api";
import youOwe from "../../docs/spec/schema/examples/commitment-you-owe.json" with { type: "json" };
import owedToYou from "../../docs/spec/schema/examples/commitment-owed-to-you.json" with { type: "json" };
import payment from "../../docs/spec/schema/examples/commitment-payment.json" with { type: "json" };
import conversation from "../../docs/spec/schema/examples/conversation.json" with { type: "json" };
import receipt from "../../docs/spec/schema/examples/record-receipt.json" with { type: "json" };
import booking from "../../docs/spec/schema/examples/record-booking.json" with { type: "json" };
import feed from "../../docs/spec/schema/examples/feed.json" with { type: "json" };
import signal from "../../docs/spec/schema/examples/signal-code.json" with { type: "json" };

export const items = [youOwe, owedToYou, payment, conversation, receipt, booking, feed, signal] as unknown as Item[];

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
      last_sync: { kind: "incremental", started: "2026-11-16T09:00:00Z", ended: "2026-11-16T09:00:02Z", result: "ok" },
    },
  ],
  messages: { total: 84212, fetched: 84212, understood: 84212, gone: 0 },
};

export const brief: Brief = {
  you_owe: of(byForm("commitment").filter((i) => i.direction === "you_owe")),
  owed_to_you: of(byForm("commitment").filter((i) => i.direction === "owed_to_you")),
  conversations: of(byForm("conversation")),
  records: { receipt: 1, booking: 1 },
  // The feed example is bundled by rule 3 ("Bundle the feed"), so it is only in its bundle.
  feed: of([]),
  signals: of(byForm("signal")),
  strangers: of([]),
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

export function message(id: string): Message | undefined {
  const item = items.find((i) => i.sources.some((s) => s.message === id));
  if (!item) return undefined;
  const source = item.sources.find((s) => s.message === id)!;
  return {
    id,
    thread: source.thread,
    date: item.created,
    from: { name: "Example Sender", address: "sender@example.com" },
    subject: item.title,
    labels: ["INBOX"],
    to: "you@example.com",
    cc: null,
    text: `${source.quote ?? item.title}\n\nThis is a fixture message, made from the open model examples.`,
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
];

export const policy: Policy = {
  mode: "observe",
  version: 3,
  next_id: 6,
  rules,
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
  "m-55211": ["t-8841", "You", "you@example.com", "Re: Q4 numbers for the board"],
  "m-60001": ["t-9002", "Dana Brooks", "dana@leadgen.example", "Quick question about your Q1 plans"],
  "m-61500": ["t-9100", "Morning Brew", "crew@morningbrew.example", "Monday: the market opens higher"],
};
const PUBLISHERS = [
  ["Morning Brew", "crew@morningbrew.example", "Your morning in markets"],
  ["Stratechery", "email@stratechery.example", "The weekly article"],
  ["The Verge", "newsletter@theverge.example", "Verge Deals of the day"],
  ["Product Hunt Daily", "hello@producthunt.example", "Top launches of the day"],
] as const;

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
];

/** The answer for one API path (no `/api/` prefix), or undefined for a 404. */
export function answer(path: string): unknown {
  const url = new URL(path, "http://local/");
  const q = url.searchParams;
  const [name, id] = url.pathname.slice(1).split("/");
  if (name === "status") return status;
  if (name === "brief" && id?.startsWith("bundle-")) {
    const bundle = brief.bundles?.find((b) => `bundle-${b.rule}` === id);
    return bundle ? { count: bundle.count, items: bundle.items, next: null } : undefined;
  }
  if (name === "records") return { receipt: 1, booking: 1 };
  if (name === "brief" && id === "hidden") return { count: hidden.length, items: hidden, next: null };
  if (name === "handled") return handled;
  if (name === "brief" && id) {
    const section = brief[id as "you_owe"];
    return section && "items" in section ? { items: section.items, next: null, duplicates: section.duplicates } : undefined;
  }
  if (name === "brief") return brief;
  if (name === "items" && id) return [...items, ...hidden].find((i) => i.id === id);
  if (name === "items") {
    return { items: items.filter((i) => (!q.get("form") || i.form === q.get("form")) && (!q.get("type") || i.type === q.get("type"))), next: null };
  }
  if (name === "messages" && id) return message(id);
  if (name === "messages" && q.get("ids")) return { messages: q.get("ids")!.split(",").map(summaryOf) };
  if (name === "changesets" && id) return changesets.find((c) => c.id === id);
  if (name === "changesets") return { changesets };
  if (name === "search") return { messages: [] };
  if (name === "policy") return policy;
  return undefined;
}
