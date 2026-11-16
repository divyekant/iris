// Answers of the local API, made from the examples of docs/spec/schema/examples.
// The dev server (`pnpm dev --mode fixtures`) and the tests use them. They are not in the build.
import type { Brief, Item, Message, Policy, Rule, Status } from "../src/api";
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
};

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
  if (name === "brief" && id) {
    const section = brief[id as "you_owe"];
    return section && "items" in section ? { items: section.items, next: null, duplicates: section.duplicates } : undefined;
  }
  if (name === "brief") return brief;
  if (name === "items" && id) return items.find((i) => i.id === id);
  if (name === "items") {
    return { items: items.filter((i) => (!q.get("form") || i.form === q.get("form")) && (!q.get("type") || i.type === q.get("type"))), next: null };
  }
  if (name === "messages" && id) return message(id);
  if (name === "search") return { messages: [] };
  if (name === "policy") return policy;
  return undefined;
}
