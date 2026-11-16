// Words for the review screen (R2-06): a changeset of spec 2 section 5.1 in
// plain sentences. Pure functions, so the tests need no DOM.
import type { Action, Changeset, Item, Target } from "./api";
import { href } from "./route";

const n = (count: number, one: string, many: string) => `${count.toLocaleString("en-US")} ${count === 1 ? one : many}`;
const arg = (a: Action, key: string) => (typeof a.args[key] === "string" ? (a.args[key] as string) : "");

/** What the action does, as the start of a sentence: "Archive", "Add the label Finance". */
export function verb(a: Action): string {
  switch (a.verb) {
    case "archive":
      return "Archive";
    case "inbox":
      return "Move to the inbox";
    case "label":
      return `Add the label “${arg(a, "label")}” to`;
    case "unlabel":
      return `Remove the label “${arg(a, "label")}” from`;
    case "read":
      return "Mark as read";
    case "unread":
      return "Mark as unread";
    case "trash":
      return "Move to the trash";
    case "untrash":
      return "Take out of the trash";
    case "close":
      return "Close";
    case "decline":
      return "Decline";
    case "defer":
      return `Defer to ${arg(a, "until") || arg(a, "date")}`;
    case "correct":
      return `Correct ${arg(a, "field") || "a field of"}`;
    default:
      return a.verb;
  }
}

/** What the action changes: the real count of messages at submit, and the threads that the author named. */
export function target(a: Action): string {
  const messages = a.resolved.messages.length;
  const threads = a.targets.threads?.length ?? 0;
  const items = a.targets.items?.length ?? 0;
  if (items) return n(items, "item", "items");
  const text = n(messages, "message", "messages");
  return threads > 1 ? `${text} in ${n(threads, "thread", "threads")}` : text;
}

/** The arguments that set the effect of a state verb (spec 2 section 4), in words. */
export function detail(a: Action): string {
  const value = a.args.value;
  switch (a.verb) {
    case "close":
    case "decline":
      return arg(a, "reason") ? `Reason: ${arg(a, "reason")}` : "";
    case "correct":
      return value === undefined ? "" : `New value: ${typeof value === "string" ? value : JSON.stringify(value)}`;
    default:
      return "";
  }
}

/** The key of an action: its number starts at 1 again in each revision. */
export const key = (a: Action) => `${a.revision}.${a.n}`;

/** One target of an action as the review shows it, or the ID of a target that is gone. */
export type Row = { id: string; gone: false; href: string; who: string; what: string; when: string | null } | { id: string; gone: true };

const FORMS: Record<string, string> = { commitment: "Commitment", conversation: "Conversation", record: "Record", feed: "Feed", signal: "Signal" };

export const messageRow = (m: Target): Row =>
  m.gone ? { id: m.id, gone: true } : { id: m.id, gone: false, href: href.mail(m.id), who: m.from.name || m.from.address || "Unknown sender", what: m.subject || "(no subject)", when: m.date };

export const itemRow = (id: string, item: Item | null): Row =>
  item ? { id, gone: false, href: href.item(id), who: FORMS[item.form] ?? item.form, what: item.title, when: null } : { id, gone: true };

/** The IDs that an action changes: its items, or the messages that submit resolved. */
export const targets = (a: Action) => (a.targets.items?.length ? a.targets.items : a.resolved.messages);

/**
 * The pages of rows of each action. One load runs at a time for each key,
 * and a row shows once, so a load that ends after another one adds nothing twice.
 */
export function pages(fetch: (ids: string[], items: boolean) => Promise<Row[]>) {
  const lists = new Map<string, Row[]>();
  const pending = new Set<string>();
  return {
    async more(a: Action, count: number): Promise<Row[] | undefined> {
      const k = key(a);
      if (pending.has(k)) return undefined;
      const have = lists.get(k) ?? [];
      const seen = new Set(have.map((r) => r.id));
      const next = targets(a).filter((id) => !seen.has(id)).slice(0, count);
      if (!next.length) return have;
      pending.add(k);
      try {
        const got = await fetch(next, !!a.targets.items?.length);
        const list = [...have, ...got.filter((r) => !seen.has(r.id) && !!seen.add(r.id))];
        lists.set(k, list);
        return list;
      } finally {
        pending.delete(k);
      }
    },
  };
}

/** Why a person must look (spec 2 section 5.3), in words. */
export const FLAGS: Record<string, string> = {
  bulk: "Changes more than 500 messages",
  first_contact: "The thread is with a sender you do not know",
  new_recipient: "Goes to an address you never wrote to",
  outside_thread: "Goes to an address that is not in the thread",
  money: "Promises an amount of money",
  date: "Promises a date",
  attachment: "Mentions an attachment",
  untrusted_content: "Holds text from another message",
};

/** A person decides an action in these states (spec 2 sections 5.2 and 5.4). */
export const waits = (a: Action) => a.decision?.state === "proposed" || a.decision?.state === "in_review";

const OPEN = ["proposed", "in_review", "approved", "held"];

/**
 * The actions to show: each submitted action of the newest revision, and an
 * action of an older revision that is still open (spec 2 section 5.5). A
 * draft action has no decision yet, and a replaced action is history.
 */
export function shown(c: Changeset): Action[] {
  return c.actions.filter((a) => a.decision && a.decision.state !== "replaced" && (a.revision === c.revision || OPEN.includes(a.decision.state)));
}

/** Who asked, for the head of the request. */
export function author(c: Changeset): string {
  if (c.author.name) return c.author.name;
  return { person: "You", agent: "An agent", triage: "Iris triage", local: "A local program" }[c.author.kind] ?? c.author.kind;
}

/** The state of an action that a person does not decide now, in words. */
export function outcome(a: Action): string {
  const d = a.decision;
  if (!d) return "";
  switch (d.state) {
    case "approved":
      return d.by === "rule" ? `Approved by rule ${d.rule}` : "You approved it";
    case "applied":
      return "Done";
    case "failed":
      return "Failed";
    case "undone":
      return "Undone";
    case "held":
      return "Held";
    case "denied":
      return d.by === "person" ? "You denied it" : `Denied: ${d.reason ?? ""}`.replace(/: $/, "");
    case "expired":
      return "Expired: no decision in 14 days";
    case "cancelled":
      return "The author withdrew it";
    default:
      return "";
  }
}

/** A short date and time for a line of the review: the year only when it is not this year. */
export function short(value: string | null | undefined, now = new Date()): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  const year = date.getFullYear() === now.getFullYear() ? undefined : "numeric";
  return date.toLocaleString("en-US", { month: "short", day: "numeric", year, hour: "numeric", minute: "2-digit" });
}

