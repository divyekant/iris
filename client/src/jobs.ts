// Words for the jobs of the agents (card R4-09): the state of a job and each
// step of its timeline, as plain sentences. Pure functions, so the tests need
// no DOM. Text from an agent or from mail is data: the view shows it as text.
import { get, post, type Job, type JobChangeset, type JobDetail, type JobEvent, type JobPage, type JobSession, type Target } from "./api";
import { access } from "./access.svelte.ts";
import { daysFrom } from "./format";
import { href } from "./route";

const n = (count: number, one: string, many: string) => `${count.toLocaleString("en-US")} ${count === 1 ? one : many}`;

/** A short day: "Wed 18 Nov". */
export const day = (value: string) => new Date(value).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
/** The short day of a due date, in the time zone of the owner (issue 892). An unknown zone: the zone of the browser. */
export function dueDay(value: string, zone: string | null | undefined = access.zone): string {
  try {
    if (zone) return new Date(value).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short", timeZone: zone });
  } catch {
    // An unknown zone.
  }
  return day(value);
}
/** A time of day: "09:20". */
export const clock = (value: string) => new Date(value).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });

const relative = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

/** The jobs of one state that the Jobs view asks for at a time: 20 for an ended job, 50 for the others. */
export const pageSize = (state: Job["state"]) => (state === "done" ? 20 : 50);

/**
 * The due date of a job, with a tone: `late` after the date, `soon` 2 days or less before it.
 * An ended job keeps its date with no tone, so the person can see if it ended before the date.
 */
export function due(job: Job, now = new Date()): { text: string; tone: "late" | "soon" | "" } | null {
  if (!job.due) return null;
  if (job.state === "done") return { text: `Was due ${dueDay(job.due)}`, tone: "" };
  // A job due date is a day of the owner zone, as the start gives it (start.ts, issue 892).
  const days = daysFrom(job.due, now);
  // The words say "late" each time the tone does, so color is not the only sign.
  if (Date.parse(job.due) <= now.getTime()) return { text: `Was due ${dueDay(job.due)}, ${days < 0 ? relative.format(days, "day") : "earlier today"}`, tone: "late" };
  return { text: `Due ${dueDay(job.due)}, ${relative.format(days, "day")}`, tone: days <= 2 ? "soon" : "" };
}

/**
 * The mail of a wait: the subject of the newest message of its thread and a link to it, or null.
 * The job does not say who must reply, and the newest message can be your own, so no sender is named.
 */
export function waitMail(threads: Record<string, Target>, thread: string | undefined): { subject: string; link: string } | null {
  const t = thread ? threads[thread] : undefined;
  if (!t || t.gone) return null;
  return { subject: t.subject || "(no subject)", link: href.mail(t.id) };
}

/** The state of a job now, in a few words, for a list line or the top of the page. */
export function now(job: Job): string {
  switch (job.state) {
    case "working":
      return "The agent works on it.";
    case "needs_you":
      // The engine also asks, on the due date (decision D24): a job page knows who, a list line does not.
      return "It asks you a question.";
    case "waiting": {
      const until = job.wait ? `, until ${day(job.wait.until)}` : "";
      return job.wait?.thread ? `Waits for a reply${until}.` : `Waits${until}.`;
    }
    case "done":
      return job.end === "cancel" ? `Cancelled ${day(job.updated)}.` : `Done ${day(job.updated)}.`;
  }
}

const ENDED: Record<NonNullable<JobSession["end"]>, string> = {
  exit: "ended",
  stop: "was stopped",
  revoke: "ended because its access was taken back",
  job_state: "ended when the job changed state",
  time_limit: "ended at its time limit",
  engine: "ended when the engine stopped",
  launch_failed: "did not start",
  helper_failed: "failed in its sandbox helper",
};

/**
 * The state of a working job on its page (issue 742), from its newest session.
 * `said` replaces the words of `now`. `ended` tells how the last session ended. `run` is the command that starts a
 * new session, with the agent of the last session (claude-code when the job had none). An ending session is still
 * live, and the engine refuses a new one, so it gives no `run`.
 * A job that does not work, or whose session starts or runs, gives null: `now` is true for it.
 */
export function agent(d: Pick<JobDetail, "job" | "session">): { said: string; ended?: string; run?: string } | null {
  const s = d.session;
  if (d.job.state !== "working" || s?.state === "starting" || s?.state === "running") return null;
  const said = "No agent works on it now.";
  if (s?.state === "ending") return { said, ended: `The last session (${s.id}, ${s.agent}) stops now. When it has stopped, you can start an agent again.` };
  const run = `iris job run --agent ${s?.agent ?? "claude-code"} ${d.job.id}`;
  if (!s || !s.end) return { said, run };
  const code = s.end === "exit" && s.exit !== null && s.exit !== 0 ? ` with exit code ${s.exit}` : "";
  return { said, ended: `The last session (${s.id}, ${s.agent}) ${ENDED[s.end]}${code}.`, run };
}

/**
 * The words of a working job of a list when no agent works on it (issue 791), from the session that the page gives.
 * Null when an agent works on it, when the job does not work, or when the engine gave no session for it.
 */
export function idle(page: Pick<JobPage, "sessions">, job: Job): { said: string; ended?: string } | null {
  const sessions = page.sessions;
  if (!sessions || !Object.hasOwn(sessions, job.id)) return null;
  const a = agent({ job: { ...job, events: [] }, session: sessions[job.id] });
  return a && { said: a.said, ended: a.ended };
}

/** A working job that no agent works on now, with the words of `idle`. */
export type Idle = { job: Job; said: string; ended?: string };

/** The working jobs that no agent works on now (issue 791), for the brief: each page of working jobs. */
export async function loadIdle(): Promise<Idle[]> {
  const out: Idle[] = [];
  let next: string | null = null;
  do {
    const page: JobPage = await get<JobPage>(`jobs?state=working&limit=200${next ? `&after=${encodeURIComponent(next)}` : ""}`);
    for (const job of page.jobs) {
      const i = job.state === "working" ? idle(page, job) : null;
      if (i) out.push({ job, ...i });
    }
    next = page.next;
  } while (next);
  return out;
}

/** One step of the timeline. `quote` is text of the agent or the person, shown as text. */
export type Step = {
  at: string;
  key: string;
  who: "you" | "agent" | "iris";
  text: string;
  quote?: string;
  mail?: { subject: string; link: string };
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
      if (e.by === "engine") return { at, key, who: "iris", text: "Iris asked you:", quote: e.question, tone: "ask" };
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
  // The newest revision is a draft: its actions are not a request yet (spec 2 section 5), so give no count.
  if (c.state === "draft") return { at: c.at, key: c.id, who: "agent", text: "The agent asked for changes. It now drafts a new version of the request.", quote: c.why ?? undefined };
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

/** Who asks the open question of a job: the engine on the due date (decision D24), else the agent. */
export function asker(job: Pick<JobDetail["job"], "state" | "events">): "Iris" | "The agent" {
  const last = job.events[job.events.length - 1];
  return job.state === "needs_you" && last?.by === "engine" ? "Iris" : "The agent";
}

/** Start a job from a typed goal, with no mail (decision D25). `request` makes a retry safe: a second start with it gives 409 and the job. */
export function startJob(goal: string, due: string | null, request: string): Promise<Job> {
  return post<Job>("jobs", { goal, due, request });
}

/** Change the goal and the due date of an open job (decision D25). The answer is the job page again. */
export async function editJob(id: string, goal: string, due: string | null): Promise<JobDetail> {
  await post<Job>(`jobs/${encodeURIComponent(id)}/edit`, { goal, due });
  return loadJob(id);
}

/**
 * A job document from the engine. A response with no job (a body that the client
 * cannot read, for example a stream that stopped) is an error with no status, as a
 * lost answer: a write before it can have committed.
 */
async function detailOf(answer: Promise<JobDetail | null>): Promise<JobDetail> {
  const d = await answer;
  if (!d || typeof d.job?.id !== "string") throw new Error("The engine gave no job.");
  return d;
}

/** End the job `id` (issue 322): `complete` (done) or `cancel`. The answer is the job, as `loadJob` gives it. */
export function endJob(id: string, end: "complete" | "cancel"): Promise<JobDetail> {
  return detailOf(post<JobDetail>(`jobs/${encodeURIComponent(id)}/finish`, { end }));
}

/** End the wait of `job` early (issue 959), for the wait that the person saw (`updated`). The answer is the job, as `loadJob` gives it. */
export function wakeJob(job: Pick<Job, "id" | "updated">): Promise<JobDetail> {
  return detailOf(post<JobDetail>(`jobs/${encodeURIComponent(job.id)}/wake`, { updated: job.updated }));
}

/** The job page of `id`, read again: for example after a change whose answer did not come. */
export function loadJob(id: string): Promise<JobDetail> {
  return detailOf(get<JobDetail>(`jobs/${encodeURIComponent(id)}`));
}

/** True when two due dates name the same instant, or both are empty. */
export function sameDue(a: string | null | undefined, b: string | null | undefined): boolean {
  return a && b ? Date.parse(a) === Date.parse(b) : !a && !b;
}

/** Add a thread to the scope of a job. `messages` are the IDs of the preview that the person saw: a change gives 409. */
export function addThread(id: string, thread: string, messages: string[]): Promise<JobDetail> {
  return post<JobDetail>(`jobs/${encodeURIComponent(id)}/scope`, { thread, messages });
}

/** Take a thread or an item out of the scope of a job. The engine ends the open session of the job. */
export function removeRef(id: string, ref: string): Promise<JobDetail> {
  return post<JobDetail>(`jobs/${encodeURIComponent(id)}/scope/remove`, { ref });
}

/** The jobs whose agent asks you a question now (card R5-05), the newest question first (`updated` of the job, not its ID). */
export async function loadQuestions(): Promise<Job[]> {
  // The API pages by job ID, so the newest question can be on any page: read each page.
  let jobs: Job[] = [];
  let next: string | null = null;
  do {
    const page: JobPage = await get<JobPage>(`jobs?state=needs_you&limit=200${next ? `&after=${encodeURIComponent(next)}` : ""}`);
    jobs = jobs.concat(page.jobs);
    next = page.next;
  } while (next);
  return jobs.filter((j) => j.state === "needs_you" && j.question).sort((a, b) => Date.parse(b.updated) - Date.parse(a.updated));
}

/** Your answer to the question of a job (card R5-05). The engine starts a new session of the job with it. */
export function answerJob(job: Pick<Job, "id" | "question" | "updated">, answer: string): Promise<JobDetail> {
  // The question and the time that the person saw: the engine refuses (409) an answer to an older question.
  return detailOf(post<JobDetail>(`jobs/${encodeURIComponent(job.id)}/answer`, { answer, question: job.question, updated: job.updated }));
}
