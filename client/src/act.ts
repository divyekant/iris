// The state verbs of the person on a commitment (spec 2 section 4): Done
// (`close`), Later (`defer`) and Decline (`decline`). The engine applies a
// verb through a changeset and the ledger (POST /api/items/{id}/{verb}).

/** The time between a click and the request, so that the person can undo a wrong click. */
export const HOLD = 5000;

export type Verb = "close" | "defer" | "decline";

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

/** The Monday after `now`. On a Monday, the Monday a week later. Never before `first(now)`. */
export function nextWeek(now: Date): string {
  const monday = ymd(plus(now, ((8 - now.getDay()) % 7) || 7));
  const min = first(now);
  return monday > min ? monday : min;
}

/** The name of the first day of Later: "Tomorrow", or the day after when tomorrow already started in UTC. */
export function firstName(now: Date): string {
  return first(now) === ymd(plus(now, 1)) ? "Tomorrow" : "In 2 days";
}

/** A day as short words: "Tue 17 Nov". */
export function day(value: string): string {
  const [y, m, d] = value.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
}

/** What the person did, as the page says it. */
export function said(verb: Verb, date?: string): string {
  if (verb === "close") return "Done.";
  if (verb === "decline") return "Declined.";
  return `Later, on ${day(date ?? "")}.`;
}

/**
 * Run `send` after `ms`, unless `cancel` runs first. `flush` sends now, for
 * example when the person leaves the page during the hold.
 */
export function hold(send: () => void, ms = HOLD): { cancel(): void; flush(): void } {
  let timer: ReturnType<typeof setTimeout> | undefined = setTimeout(go, ms);
  function go() {
    if (timer === undefined) return;
    clearTimeout(timer);
    timer = undefined;
    send();
  }
  return {
    cancel() {
      clearTimeout(timer);
      timer = undefined;
    },
    flush: go,
  };
}
