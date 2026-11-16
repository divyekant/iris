// Words for the review screen (R2-06): a changeset of spec 2 section 5.1 in
// plain sentences. Pure functions, so the tests need no DOM.
import type { Action, Changeset } from "./api";

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

