// The measures of the manifesto, "How we measure Iris" (card R5-12). The engine
// computes them on this machine (GET /api/measures). Nothing leaves the machine.
import { post } from "./api";

export type Measures = {
  days: { day: string; opens: number }[];
  /** `null` before the first brief. Each field is `null` when the store is older than the measure. */
  first_brief: { connect: string | null; brief: string | null; seconds: number | null } | null;
  /** Each account. An account with no check has `checked: null` and `complete: false`. */
  mirror: { account: string; checked: string | null; differences: number | null; complete: boolean }[];
  memory: { rss_kib: number | null; messages: number };
  approvals: { decided: number; approved: number };
  undo: { applied: number; undone: number };
};

/** met: meets its target. missed: misses it. flat: no fixed target. none: no data yet. */
export type Tone = "met" | "missed" | "flat" | "none";
export type Row = { name: string; value: string; detail: string; tone: Tone };
export type Bar = { day: string; opens: number | null };

/** The number of days that the page shows. */
export const SPAN = 14;
const MAX_RSS_KIB = 50 * 1024;
const MAX_FIRST_BRIEF = 600;
const TARGET_MESSAGES = 100_000;

/** The local date of `now`, YYYY-MM-DD. */
export function localDay(now = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}`;
}

let use: { day: string; done: Promise<void> } | null = null;
const pending = new Set<Promise<void>>();
// The opens that got no answer of success. A 400 is a definite refusal (for example a
// day that is too old now), so it is not kept. The engine counts each mail once for each
// day, so a second try of an open that the engine wrote but whose answer got lost
// changes nothing.
type Open = { day: string; message: string };
const failed: Open[] = [];

function send(body: { day: string; message?: string }): Promise<void> {
  const open = body.message !== undefined;
  const done: Promise<void> = post(`measures/${open ? "open" : "use"}`, body).then(
    () => {},
    (e: { status?: number }) => {
      if (open && e?.status !== 400) failed.push(body as Open);
      if (use?.done === done) use = null;
    },
  );
  pending.add(done);
  void done.then(() => pending.delete(done));
  return done;
}

/** Send each open again that got no answer of success. */
function resend() {
  for (const open of failed.splice(0)) void send(open);
}

/**
 * Count one use of the web client on the local day, or the open of one source mail
 * (`{ open: <m- ID> }`). A use counts once for each day in this page: a second call of
 * the same day gives the promise of the first. A failed count does not stop the page.
 * The next use tries again, and each failed open goes again with the next count.
 */
export function count(event: "use" | { open: string }, now = new Date()): Promise<void> {
  const day = localDay(now);
  resend();
  if (event !== "use") return send({ day, message: event.open });
  if (use?.day === day) return use.done;
  const done = send({ day });
  use = { day, done };
  return done;
}

/** Send the failed opens again, then wait until each count of this page has its answer, so a read of the measures includes them. */
export async function written(): Promise<void> {
  resend();
  while (pending.size > 0) await Promise.all([...pending]);
}

/** The last SPAN local days, the oldest first. A day with no use has `opens: null`. */
export function bars(m: Measures, now = new Date()): Bar[] {
  const opens = new Map(m.days.map((d) => [d.day, d.opens]));
  return Array.from({ length: SPAN }, (_, i) => {
    const day = localDay(new Date(now.getFullYear(), now.getMonth(), now.getDate() - (SPAN - 1 - i)));
    return { day, opens: opens.get(day) ?? null };
  });
}

/** The days of the window since the first use: Iris cannot be used before its first day. */
function since(m: Measures, shown: Bar[]): Bar[] {
  const first = m.days.reduce((a, d) => (d.day < a ? d.day : a), shown[shown.length - 1].day);
  return shown.filter((b) => b.day >= first);
}

const num = (n: number) => n.toLocaleString("en-US");
const percent = (part: number, all: number) => `${Math.round((part / all) * 100)}%`;
const plural = (n: number, one: string, many: string) => `${num(n)} ${n === 1 ? one : many}`;
/** "3 Nov". A date with no time is a local day. */
export function shortDay(value: string): string {
  const d = /^\d{4}-\d{2}-\d{2}$/.test(value) ? new Date(+value.slice(0, 4), +value.slice(5, 7) - 1, +value.slice(8)) : new Date(value);
  return d.toLocaleDateString("en-GB", { day: "numeric", month: "short" });
}

/** One row for each measure other than the opens, in the order of the manifesto. */
export function rows(m: Measures, now = new Date()): Row[] {
  const window = since(m, bars(m, now));
  const used = window.filter((b) => b.opens !== null).length;
  const brief = m.first_brief;
  // The target is met only when each account has a complete check with no difference.
  const checks = m.mirror.filter((c) => c.checked !== null) as { checked: string; differences: number | null; complete: boolean }[];
  const unchecked = m.mirror.length - checks.length;
  const differences = checks.reduce((a, c) => a + (c.differences ?? 0), 0);
  const stopped = checks.some((c) => !c.complete || c.differences === null);
  const oldest = checks.reduce((a, c) => (c.checked < a ? c.checked : a), checks[0]?.checked ?? "");
  const rss = m.memory.rss_kib;
  const { decided, approved } = m.approvals;
  const { applied, undone } = m.undo;
  return [
    m.days.length === 0
      ? { name: "Days you used Iris", value: "Not yet", detail: "Target: each day.", tone: "none" }
      : {
          name: "Days you used Iris",
          value: `${num(used)} of ${num(window.length)}`,
          detail: `In the last ${plural(window.length, "day", "days")}. Target: each day.`,
          tone: used === window.length ? "met" : "missed",
        },
    brief?.seconds != null && brief.connect
      ? {
          name: "Time to the first brief",
          value: brief.seconds < 60 ? "Under 1 min" : `${num(Math.ceil(brief.seconds / 60))} min`,
          detail: `From the connect step on ${shortDay(brief.connect)}. Target: 10 minutes or less.`,
          tone: brief.seconds <= MAX_FIRST_BRIEF ? "met" : "missed",
        }
      : brief
        ? { name: "Time to the first brief", value: "Not known", detail: "Your Iris data is older than this measure. Target: 10 minutes or less.", tone: "none" }
        : { name: "Time to the first brief", value: "Not yet", detail: "Iris measures it at the first brief. Target: 10 minutes or less.", tone: "none" },
    checks.length === 0
      ? { name: "Differences from Gmail", value: "Not yet", detail: "Run iris mirror verify to compare the mirror with Gmail. Target: zero.", tone: "none" }
      : {
          name: "Differences from Gmail",
          value: num(differences),
          detail: `${unchecked ? `${plural(unchecked, "account has", "accounts have")} no check yet. ` : ""}${stopped ? "A check stopped before its end. " : ""}${checks.length === 1 ? "Last check" : `Last checks of ${checks.length} accounts, the oldest`} on ${shortDay(oldest)}. Target: zero.`,
          tone: differences > 0 ? "missed" : stopped || unchecked ? "flat" : "met",
        },
    rss === null
      ? { name: "Memory of the engine", value: "Not known", detail: "The system did not give the memory of the engine. Target: 50 MB or less with 100,000 messages.", tone: "none" }
      : {
          name: "Memory of the engine",
          value: `${num(Math.round(rss / 1024))} MB`,
          detail: `With ${plural(m.memory.messages, "message", "messages")}. Target: 50 MB or less with 100,000.`,
          // The target is for 100,000 messages: under 50 MB with more meets it, over 50 MB with fewer misses it.
          tone: rss <= MAX_RSS_KIB ? (m.memory.messages >= TARGET_MESSAGES ? "met" : "flat") : m.memory.messages <= TARGET_MESSAGES ? "missed" : "flat",
        },
    decided === 0
      ? { name: "Changes you approved as proposed", value: "Not yet", detail: "You have no decision on a proposed change yet. Higher is better.", tone: "none" }
      : { name: "Changes you approved as proposed", value: `${num(approved)} of ${num(decided)}`, detail: `${percent(approved, decided)}. Higher is better.`, tone: "flat" },
    applied === 0
      ? { name: "Changes that you undid", value: "Not yet", detail: "Iris has applied no change yet. Lower is better.", tone: "none" }
      : { name: "Changes that you undid", value: `${num(undone)} of ${num(applied)}`, detail: `${percent(undone, applied)}. Lower is better.`, tone: "flat" },
    { name: "Sends without review after hostile mail", value: "Not yet", detail: "Iris does not measure this yet. Target: zero.", tone: "none" },
  ];
}

/** The opening sentence: the opens of today, then the measures that have a target. */
export function lead(m: Measures, now = new Date()): { opens: number; text: string } {
  const opens = m.days.find((d) => d.day === localDay(now))?.opens ?? 0;
  const all = rows(m, now);
  const missed = all.filter((r) => r.tone === "missed");
  const targets = all.filter((r) => r.tone === "met" || r.tone === "missed").length;
  const text =
    targets === 0
      ? ""
      : missed.length === 0
        ? "Each measure with a target meets it."
        : missed.length === 1
          ? `One measure misses its target: ${missed[0].name}.`
          : `${num(missed.length)} measures miss their target.`;
  return { opens, text };
}

/** The opens of the days of use in the window, as "1.6 a day". */
export function average(m: Measures, now = new Date()): { perDay: string; days: number } {
  const used = bars(m, now).filter((b) => b.opens !== null) as { day: string; opens: number }[];
  const sum = used.reduce((a, b) => a + b.opens, 0);
  return { perDay: used.length ? (sum / used.length).toLocaleString("en-US", { maximumFractionDigits: 1 }) : "0", days: used.length };
}
