// The words of the history view (card R2-07): one ledger entry as a sentence.
import type { LedgerEntry } from "./api";

const arg = (e: LedgerEntry, key: string) => (typeof e.args?.[key] === "string" ? (e.args[key] as string) : "");
const count = (n: number, one: string, many: string) => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;

/** What Iris did, in the past tense: "Archived", "Added the label “Finance” to". */
export function did(e: LedgerEntry): string {
  switch (e.verb) {
    case "archive":
      return "Archived";
    case "inbox":
      return "Moved to the inbox";
    case "label":
      return `Added the label “${arg(e, "label")}” to`;
    case "unlabel":
      return `Removed the label “${arg(e, "label")}” from`;
    case "read":
      return "Marked as read";
    case "unread":
      return "Marked as unread";
    case "trash":
      return "Moved to the trash";
    case "untrash":
      return "Took out of the trash";
    case "close":
      return "Closed";
    case "decline":
      return "Declined";
    case "defer":
      return `Deferred to ${arg(e, "until") || arg(e, "date")}`;
    case "correct":
      return "Corrected";
    case "skip":
      return "Skipped";
    case "mute":
      return "Muted";
    case "dismiss":
      return "Dismissed";
    case "restore":
      return "Restored";
    case "send":
      return "Sent";
    case "draft":
      return "Wrote the draft of";
    default:
      // A verb that the changeset schema does not have yet: test/ledger.test.mjs checks each one.
      return e.verb;
  }
}

/** What the entry changed: its messages, its item, or its mail. */
export function what(e: LedgerEntry): string {
  if (e.verb === "send" || e.verb === "draft") return "a mail";
  if (e.items === 1 && e.title) return `“${e.title}”`;
  if (e.items > 0) return count(e.items, "item", "items");
  return count(e.messages, "message", "messages");
}

/**
 * Who decided the entry: the author, and the rule or the person that approved it. A rule
 * decision uses the reason that the engine recorded with it, because a rule can get a new
 * name after the decision (spec 2 section 6.3).
 */
export function who(e: LedgerEntry): string {
  const a = e.author;
  const author =
    a.kind === "person" ? "You" : a.kind === "agent" ? (a.goal ? `An agent, job “${a.goal}”` : "An agent") : a.kind === "triage" ? "Iris triage" : (a.name ?? "A local tool");
  if (e.decided_by === "rule") return e.reason ? `${author}. ${e.reason}` : `${author}, allowed by your rule ${e.rule}`;
  return a.kind !== "person" && e.decided_by === "person" ? `${author}, approved by you` : author;
}

/** The state of an entry that is not plainly done, or "". */
export function status(e: LedgerEntry): string {
  if (e.state === "writing") return "Iris makes this change now.";
  if (e.state === "failed") {
    const part = e.messages > 0 ? ` ${e.changed.toLocaleString("en-US")} of ${count(e.messages, "message", "messages")} changed.` : "";
    return `Failed.${part}${e.error ? ` ${e.error}` : ""}`;
  }
  return "";
}

/** The day heading of a time: "Today", "Yesterday", or the date, in the local day of `now`. */
export function day(at: string, now = new Date()): string {
  const d = new Date(at);
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((start(now) - start(d)) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short", ...(d.getFullYear() === now.getFullYear() ? {} : { year: "numeric" }) });
}

/** The local time of day: "9:42". */
export const time = (at: string) => new Date(at).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });

/** The entries in groups of one local day each, in the order of the list. */
export function byDay(entries: LedgerEntry[], now = new Date()): { day: string; entries: LedgerEntry[] }[] {
  const groups: { day: string; entries: LedgerEntry[] }[] = [];
  for (const e of entries) {
    const d = day(e.at, now);
    if (groups.at(-1)?.day === d) groups.at(-1)!.entries.push(e);
    else groups.push({ day: d, entries: [e] });
  }
  return groups;
}
