// The state verbs of the person (spec 2 section 4): on a commitment, Done
// (`close`), Later (`defer`) and Decline (`decline`); on a feed item, a
// conversation or a signal, Skip, Mute or Dismiss (R5-02). The engine applies
// a verb through a changeset and the ledger (POST /api/items/{id}/{verb}).

/** The time between a click and the request, so that the person can undo a wrong click. */
export const HOLD = 5000;

export type Verb = "close" | "defer" | "decline" | "skip" | "mute" | "dismiss";

/** The one verb that takes an item out of the brief, with its button label, or null (spec 1 section 6.8). */
export function quiet(item: { form: string; status: string }): { verb: Verb; label: string } | null {
  if (item.form === "feed" && (item.status === "new" || item.status === "read")) return { verb: "skip", label: "Skip" };
  if (item.form === "conversation" && (item.status === "active" || item.status === "quiet")) return { verb: "mute", label: "Mute" };
  if (item.form === "signal" && item.status === "active") return { verb: "dismiss", label: "Dismiss" };
  return null;
}

/** The local day of `d` as `YYYY-MM-DD`. */
export function ymd(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function plus(now: Date, days: number): Date {
  return new Date(now.getFullYear(), now.getMonth(), now.getDate() + days);
}

/**
 * The first day that Later can give: the local day after `now`, or a later
 * day if that day already started in UTC. The engine opens a deferred
 * commitment again when its date starts in UTC (spec 1 section 6.8, issue
 * #121), so an earlier day would open it again at once.
 */
export function first(now: Date): string {
  const local = ymd(plus(now, 1));
  const utc = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() + 1)).toISOString().slice(0, 10);
  return local > utc ? local : utc;
}

/**
 * The Monday after `now`. On a Monday, the Monday a week later. Null when
 * that Monday already started in UTC (a Sunday evening west of UTC).
 */
export function nextWeek(now: Date): string | null {
  const monday = ymd(plus(now, ((8 - now.getDay()) % 7) || 7));
  return started(monday, now) ? null : monday;
}

/** True when the day `date` (`YYYY-MM-DD`) started in UTC at `now`, so a defer to it would open the item again at once. */
export function started(date: string, now: Date): boolean {
  return date < first(now);
}

/** The name of the first day of Later: "Tomorrow", or the day after when tomorrow already started in UTC. */
export function firstName(now: Date): string {
  return first(now) === ymd(plus(now, 1)) ? "Tomorrow" : "In 2 days";
}

/** A day as short words: "Tue 17 Nov". */
export function day(value: string): string {
  const [y, m, d] = value.split("-").map(Number);
  // Not new Date(y, ...): it maps the years 0 to 99 to 1900 to 1999.
  const date = new Date(2000, 0, 1);
  date.setFullYear(y, m - 1, d);
  return date.toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
}

/** What the person did, as the page says it. */
export function said(verb: Verb, date?: string): string {
  if (verb === "close") return "Done.";
  if (verb === "decline") return "Declined.";
  if (verb === "skip") return "Skipped.";
  if (verb === "mute") return "Muted.";
  if (verb === "dismiss") return "Dismissed.";
  return `Later, on ${day(date ?? "")}.`;
}

/**
 * Run `send` after `ms`, unless `cancel` runs first. `flush` sends now, for
 * example when the person leaves the page during the hold.
 */
export function hold(send: () => void, ms = HOLD): { cancel(): void; flush(): void } {
  let timer: ReturnType<typeof setTimeout> | undefined = setTimeout(go, ms);
  holds.add(go);
  function go() {
    holds.delete(go);
    if (timer === undefined) return;
    clearTimeout(timer);
    timer = undefined;
    send();
  }
  return {
    cancel() {
      holds.delete(go);
      clearTimeout(timer);
      timer = undefined;
    },
    flush: go,
  };
}

// Each hold that has not sent yet. A view whose rows can leave while a hold runs
// (the brief) sends them all when the person leaves the view, not when one row goes.
const holds = new Set<() => void>();

/** Send each hold that waits now: the person leaves the view or the page. */
export function flushHolds(): void {
  for (const go of [...holds]) go();
}
