// "Handle this" (card R4-10, docs/specs/2026-11-17-start-job.md): the start
// of a job from a commitment or a conversation, and the grants inside a job.
import type { Item, Rule, Summary } from "./api";
import { ymd } from "./act";

/** One message that a start grants (GET /api/items/{id}/job). */
export type StartMessage = Summary & { readable: boolean };
export type Preview = { messages: StartMessage[] };

/** The statuses of an item that can start a job: the same as the engine checks. */
export function canStart(item: Item): boolean {
  return (item.form === "commitment" && item.status === "open") || (item.form === "conversation" && (item.status === "active" || item.status === "quiet"));
}

/** The end of the local day `date` (`YYYY-MM-DD`), as RFC 3339 with the local offset. */
export function endOfDay(date: string): string {
  const [y, m, d] = date.split("-").map(Number);
  const t = new Date(2000, 0, 1, 23, 59, 59, 999);
  t.setFullYear(y, m - 1, d);
  const off = -t.getTimezoneOffset();
  const pad = (n: number, w = 2) => String(Math.abs(n)).padStart(w, "0");
  const zone = `${off < 0 ? "-" : "+"}${pad(Math.trunc(off / 60))}:${pad(off % 60)}`;
  return `${ymd(t)}T23:59:59.999${zone}`;
}

/** The date of the due field of the form: the local day of the due date of a commitment, or "". */
export function dueDate(item: Item): string {
  if (item.due?.at) return ymd(new Date(item.due.at));
  return item.due?.date ?? "";
}

/** The `due` of the start: the instant of the commitment if the person kept its day, else the end of the picked day, or null. */
export function dueValue(item: Item, field: string): string | null {
  if (!field) return null;
  if (item.due?.at && field === dueDate(item)) return item.due.at;
  return endOfDay(field);
}

/** The messages of a start, grouped by thread, in the order of the list. */
export function byThread(messages: StartMessage[]): { thread: string; subject: string; messages: StartMessage[] }[] {
  const out: { thread: string; subject: string; messages: StartMessage[] }[] = [];
  for (const m of messages) {
    const last = out[out.length - 1];
    if (last && last.thread === m.thread) last.messages.push(m);
    else out.push({ thread: m.thread, subject: m.subject ?? "(no subject)", messages: [m] });
  }
  return out;
}

/** "3 messages in 2 threads". */
export function reads(messages: StartMessage[]): string {
  const threads = new Set(messages.map((m) => m.thread)).size;
  const n = (k: number, one: string) => `${k} ${one}${k === 1 ? "" : "s"}`;
  return `${n(messages.length, "message")} in ${n(threads, "thread")}`;
}

/** The groups of verbs that the job page offers as grants. */
export const GRANTS = [
  { key: "tidy", title: "Tidy the mail", note: "Archive, add or remove labels, mark as read or unread, move back to the inbox", verbs: ["archive", "label", "unlabel", "read", "unread", "inbox"] },
  { key: "commitments", title: "Update commitments", note: "Mark as done, put off to a later day, decline", verbs: ["close", "defer", "decline"] },
  { key: "trash", title: "Move mail to the trash", note: "You can take it back from the trash for 30 days", verbs: ["trash"] },
] as const;

/** The grants of `job` in the active policy: the allow rules with this job. */
export function grantsOf(rules: Rule[], job: string): Rule[] {
  return rules.filter((r) => r.job === job && r.effect === "allow");
}

/** The rule of a group: a grant of the job with the same verbs, or undefined. */
export function ruleOf(rules: Rule[], group: { verbs: readonly string[] }): Rule | undefined {
  const want = [...group.verbs].sort().join();
  return rules.find((r) => [...r.verbs].sort().join() === want);
}
