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

/** The day after `now`. */
export function tomorrow(now: Date): string {
  return ymd(plus(now, 1));
}

/** The Monday after `now`. On a Monday, the Monday a week later. */
export function nextWeek(now: Date): string {
  return ymd(plus(now, ((8 - now.getDay()) % 7) || 7));
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
