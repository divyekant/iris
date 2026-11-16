// Words for the jobs of the agents (card R4-09): the state of a job and each
// step of its timeline, as plain sentences. Pure functions, so the tests need
// no DOM. Text from an agent or from mail is data: the view shows it as text.
import type { Job, JobChangeset, JobDetail, JobEvent, Target } from "./api";
import { daysFrom } from "./format";
import { href } from "./route";

const n = (count: number, one: string, many: string) => `${count.toLocaleString("en-US")} ${count === 1 ? one : many}`;

/** A short day: "Wed 18 Nov". */
export const day = (value: string) => new Date(value).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
/** A time of day: "09:20". */
export const clock = (value: string) => new Date(value).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });

const relative = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

/** The due date of an open job, with a tone: `late` after the date, `soon` 2 days or less before it. */
export function due(job: Job, now = new Date()): { text: string; tone: "late" | "soon" | "" } | null {
  if (!job.due || job.state === "done") return null;
  const days = daysFrom(job.due, now);
  const late = Date.parse(job.due) <= now.getTime();
  return { text: `Due ${day(job.due)}, ${relative.format(days, "day")}`, tone: late ? "late" : days <= 2 ? "soon" : "" };
}

/** Who sent the newest mail of a thread, or null when the thread is gone or unknown. */
function sender(t: Target | undefined): string | null {
  if (!t || t.gone) return null;
  return t.from.name || t.from.address || null;
}

/** The mail of a wait: a link to the newest message of its thread, or null. */
export function waitMail(threads: Record<string, Target>, thread: string | undefined): { subject: string; from: string; link: string } | null {
  const t = thread ? threads[thread] : undefined;
  if (!t || t.gone) return null;
  return { subject: t.subject || "(no subject)", from: sender(t) ?? "", link: href.mail(t.id) };
}

/** The state of a job now, in a few words, for a list line or the top of the page. */
export function now(job: Job, threads: Record<string, Target> = {}): string {
  switch (job.state) {
    case "working":
      return "The agent works on it.";
    case "needs_you":
      return "The agent asks you a question.";
    case "waiting": {
      const from = sender(job.wait?.thread ? threads[job.wait.thread] : undefined);
      const until = job.wait ? `, until ${day(job.wait.until)}` : "";
      if (from) return `Waits for a reply from ${from}${until}.`;
      return job.wait?.thread ? `Waits for a reply${until}.` : `Waits${until}.`;
    }
    case "done":
      return job.end === "cancel" ? `Cancelled ${day(job.updated)}.` : `Done ${day(job.updated)}.`;
  }
}

/** One step of the timeline. `quote` is text of the agent or the person, shown as text. */
export type Step = {
  at: string;
  key: string;
  who: "you" | "agent" | "iris";
  text: string;
  quote?: string;
  mail?: { subject: string; from: string; link: string };
  link?: { href: string; text: string };
  /** The tone of the step: a question to you, an end, or neither. */
  tone?: "ask" | "done" | "";
};

const ACTION = {
  proposed: "waits for your review",
  in_review: "waits for your review",
  approved: "approved",
  held: "on hold",
  applied: "done",
  failed: "failed",
  denied: "denied",
  expired: "expired",
  cancelled: "cancelled",
  undone: "undone",
  draft: "in a draft",
} as Record<string, string>;

/** The actions of a changeset, as "2 done, 1 waits for your review". A replaced action is not counted. */
export function actions(c: JobChangeset): string {
  return Object.entries(c.actions)
    .filter(([state]) => state !== "replaced")
    .map(([state, count]) => `${count.toLocaleString("en-US")} ${ACTION[state] ?? state}`)
    .join(", ");
}

function eventStep(e: JobEvent, before: JobEvent | undefined, i: number, threads: Record<string, Target>): Step {
  const at = e.at;
  const key = `e${i}`;
  const why = e.why;
  if (!before) return { at, key, who: "you", text: "You gave the goal.", quote: why };
  switch (e.state) {
    case "waiting": {
      const until = e.wait ? ` until ${day(e.wait.until)}` : "";
      const mail = waitMail(threads, e.wait?.thread);
      const text = mail ? `The agent waits for a reply${until}.` : e.wait?.thread ? `The agent waits for a reply${until}. The mail is gone.` : `The agent waits${until}.`;
      return { at, key, who: "agent", text, quote: why, mail: mail ?? undefined };
    }
    case "needs_you":
      return { at, key, who: "agent", text: "The agent asked you:", quote: e.question, tone: "ask" };
    case "working":
      if (e.answer !== undefined) return { at, key, who: "you", text: "You answered:", quote: e.answer };
      if (e.by === "person") return { at, key, who: "you", text: "You ended the wait.", quote: why };
      if (why === "reply") return { at, key, who: "iris", text: "A reply came. The agent works again." };
      if (why === "time") return { at, key, who: "iris", text: "The wait ended. The agent works again." };
      return { at, key, who: "iris", text: "Iris ended the wait.", quote: why };
    case "done":
      if (e.end === "cancel") return { at, key, who: "you", text: "You cancelled the job.", quote: why, tone: "done" };
      return e.by === "agent"
        ? { at, key, who: "agent", text: "The agent completed the goal.", quote: why, tone: "done" }
        : { at, key, who: "you", text: "You marked the goal done.", quote: why, tone: "done" };
  }
}

function changesetStep(c: JobChangeset): Step {
  const count = Object.entries(c.actions)
    .filter(([state]) => state !== "replaced")
    .reduce((sum, [, k]) => sum + k, 0);
  const waits = (c.actions.proposed ?? 0) + (c.actions.in_review ?? 0);
  const text = `The agent asked for ${n(count, "change", "changes")}: ${actions(c)}.`;
  const link = waits > 0 && c.state === "open" ? { href: "#/review", text: "Review" } : undefined;
  return { at: c.at, key: c.id, who: "agent", text, quote: c.why ?? undefined, link };
}

/** Each step of a job in order: the events and the changesets, by time. An event comes before a changeset of the same time. */
export function steps(d: JobDetail): Step[] {
  const events = d.job.events.map((e, i) => eventStep(e, d.job.events[i - 1], i, d.threads));
  const changes = d.changesets.map(changesetStep);
  const out: Step[] = [];
  let c = 0;
  for (const e of events) {
    while (c < changes.length && Date.parse(changes[c].at) < Date.parse(e.at)) out.push(changes[c++]);
    out.push(e);
  }
  return [...out, ...changes.slice(c)];
}
