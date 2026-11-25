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

/**
 * The day of `now` as `YYYY-MM-DD` in the time zone `zone` (an IANA name):
 * the time zone of the owner, from the engine (`iris zone`, decision D1).
 * With no zone, or a zone that the browser does not know, the zone of the
 * browser.
 */
export function today(now: Date, zone?: string | null): string {
  if (zone) {
    try {
      const parts = new Intl.DateTimeFormat("en-US", { timeZone: zone, year: "numeric", month: "2-digit", day: "2-digit" }).formatToParts(now);
      const part = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
      return `${part("year").padStart(4, "0")}-${part("month")}-${part("day")}`;
    } catch {
      // An unknown zone: the zone of the browser.
    }
  }
  return ymd(now);
}

const DAY = 86_400_000;
const clocks = new Map<string, Intl.DateTimeFormat>();

/** The UTC offset of the zone `zone` at the instant `t`, in milliseconds, or null for no zone or a zone that the browser does not know. */
export function offsetOf(t: number, zone?: string | null): number | null {
  if (!zone) return null;
  try {
    let f = clocks.get(zone);
    if (!f) {
      f = new Intl.DateTimeFormat("en-US", { timeZone: zone, hourCycle: "h23", year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric", second: "numeric" });
      clocks.set(zone, f);
    }
    const parts = f.formatToParts(t);
    const n = (type: string) => Number(parts.find((p) => p.type === type)?.value ?? 0);
    const wall = new Date(0);
    wall.setUTCFullYear(n("year"), n("month") - 1, n("day"));
    wall.setUTCHours(n("hour"), n("minute"), n("second"));
    return wall.getTime() - Math.floor(t / 1000) * 1000;
  } catch {
    return null;
  }
}

/**
 * The instant of the wall time `ms` after the start of `date` (`YYYY-MM-DD`)
 * in the zone `zone`, or null for no zone or an unknown zone. For a wall time
 * that the clock skips, the first instant after the gap. For a wall time that
 * comes two times, the first one, or the last one with `last`.
 */
export function instantOf(date: string, ms: number, zone?: string | null, last = false): number | null {
  const [y, m, d] = date.split("-").map(Number);
  const start = new Date(0);
  start.setUTCFullYear(y, m - 1, d);
  const wall = start.getTime() + ms;
  // A clock change is days away from another one: the offsets a day before and a day after are the two that can apply.
  const before = offsetOf(wall - DAY, zone);
  const after = offsetOf(wall + DAY, zone);
  if (before === null || after === null) return null;
  const fits = [wall - before, wall - after].sort((a, b) => (last ? b - a : a - b)).find((t) => t + (offsetOf(t, zone) ?? 0) === wall);
  return fits ?? wall - before;
}

/** The first instant of the day after `now` in the zone `zone`, or null for no zone or an unknown zone. */
export function startOfNextDay(now: Date, zone?: string | null): number | null {
  return instantOf(plus(today(now, zone), 1).date, 0, zone);
}

/** The day `days` after `date` (`YYYY-MM-DD`), and its weekday (0 is Sunday). */
function plus(date: string, days: number): { date: string; weekday: number } {
  const [y, m, d] = date.split("-").map(Number);
  const t = new Date(0);
  // Not Date.UTC(y, ...): it maps the years 0 to 99 to 1900 to 1999.
  t.setUTCFullYear(y, m - 1, d + days);
  const pad = (n: number, w = 2) => String(n).padStart(w, "0");
  return { date: `${pad(t.getUTCFullYear(), 4)}-${pad(t.getUTCMonth() + 1)}-${pad(t.getUTCDate())}`, weekday: t.getUTCDay() };
}

/**
 * The first day that Later can give: the day after `now` in the time zone
 * of the owner. The engine opens a deferred commitment again when its day
 * starts in that zone (spec 1 section 6.8), so an earlier day would open it
 * again at once.
 */
export function first(now: Date, zone?: string | null): string {
  return plus(today(now, zone), 1).date;
}

/** The Monday after `now` in the time zone of the owner. On a Monday, the Monday a week later. */
export function nextWeek(now: Date, zone?: string | null): string {
  const day = today(now, zone);
  return plus(day, ((8 - plus(day, 0).weekday) % 7) || 7).date;
}

/** True when the day `date` (`YYYY-MM-DD`) started in the time zone of the owner at `now`, so a defer to it would open the item again at once. */
export function started(date: string, now: Date, zone?: string | null): boolean {
  return date < first(now, zone);
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
