import { access } from "./access.svelte.ts";
import { accountNames, addPage } from "./format";
import { active, localDay } from "./measures";
// The client reads the local API of the engine (crates/iris-api/README.md).
// The form documents follow docs/spec/schema. The client only reads, except
// the policy changes of the policy view (spec 2 section 6.3), the decisions of
// the review, and the state verbs of a commitment (spec 2 section 4).

export type Party = { kind: "self" | "sender"; sender?: string; name?: string; address?: string };
export type Money = { value: string; currency: string };
export type Source = { message: string; thread: string; role: "origin" | "update" | "evidence"; quote?: string };

export type Item = {
  id: string;
  form: "commitment" | "conversation" | "record" | "feed" | "signal";
  status: string;
  title: string;
  sources: Source[];
  created: string;
  updated: string;
  derived: { by: string; rule?: string | number; model?: string };
  /** The effect of the policy on the brief (spec 2 section 6.4), with the reason. */
  attention?: { state: "shown" | "hidden" | "bundled"; rule?: number; reason?: string };
  confidence?: number;
  // Fields of each form. They are optional here, so one type holds all forms.
  direction?: "you_owe" | "owed_to_you";
  kind?: string;
  owner?: Party;
  counterparty?: Party;
  due?: { date?: string; at?: string; strict?: boolean };
  /** The day a deferred commitment opens again. */
  deferred_until?: string;
  amount?: Money;
  participants?: Party[];
  /** The mail thread of a conversation (`t-N`). */
  thread?: string;
  latest?: { at: string; from: Party; snippet: string };
  messages?: number;
  new?: number;
  your_turn?: boolean;
  summary?: string;
  /** What Iris prepared for the item (card R3-09): "draft" is a reply draft that is not stale, "summary" a summary of its long thread for an item that needs the person (issue 478). Only in the lists of items: the brief sections and pages, the gate and /api/items. */
  ready?: ("draft" | "summary")[];
  type?: string;
  issuer?: Party;
  fields?: Record<string, unknown>;
  publisher?: Party;
  list?: string;
  published?: string;
  code?: string;
  expires?: string;
  /** The confirmed matter that keeps the item (`g-4`), or null. */
  matter?: string | null;
  /** The account (the mailbox) of the item (`a-2`). Open model 1.1. */
  account?: string;
};

// A matter: mail that belongs together, as Iris proposes it and the person decides it
// (docs/spec/schema/matter.schema.json).
export type MatterForm = "commitment" | "conversation" | "record" | "signal";
export type Matter = {
  id: string;
  title: string;
  status: "proposed" | "confirmed" | "declined";
  items: string[];
  /** `signal` since open model 1.2: an older engine leaves it out. */
  counts: Record<Exclude<MatterForm, "signal">, number> & { signal?: number };
  created: string;
  updated: string;
  decided?: string;
  /** The items that Iris proposes to add to a confirmed matter (open model 1.2). */
  additions?: string[];
  derived?: { by: "model" | "agent" | "person"; model?: string; prompt?: string };
};
export type MatterList = { proposed: Matter[]; confirmed: Matter[] };
export type MatterView = {
  matter: Matter;
  items: Record<Exclude<MatterForm, "signal">, Item[]> & { signal?: Item[] };
  /** The documents of the additions of a confirmed matter, newest first. */
  additions?: Item[];
};

// `duplicates` maps an item ID to newer items that can be the same thing (spec 1
// section 7.5). The newer items are not in `items`: the client shows a pair as one line.
export type Section = { count: number; items: Item[]; duplicates?: Record<string, string[]> };

export type Brief = {
  you_owe: Section;
  owed_to_you: Section;
  conversations: Section;
  records: Record<string, number>;
  feed: Section;
  signals: Section;
  strangers: Section;
  hostile: number;
  not_understood: number;
  /** While the first sync runs: the brief covers the mail from this date (spec 1 section 4.2). */
  oldest_fetched?: string | null;
  /** The stored triage proposal: the messages of the triage actions that wait for your review, for each verb. */
  proposal?: { count: number; verbs: Record<string, number> } | null;
  /** What Iris did without you: `count` is `hidden` and the messages of the actions that a rule approved (`verbs`). */
  handled?: { count: number; verbs: Record<string, number> };
  /** One entry for each `bundle` rule that bundles items (spec 2 section 6.4): the newest items, at most 20. */
  bundles?: Bundle[];
  /** The count of items that a `hide` rule leaves out of the brief. */
  hidden?: number;
  /** The open changesets, and their actions that wait for a person (spec 2 section 5.5). */
  review?: { changesets: number; actions: number };
  /** The senders that wait at the stranger gate: the first 20 (spec 1 section 6.7). */
  gate?: GatePage;
  /** The count of senders that you ignored at the gate. */
  declined?: number;
};

export type Bundle = Section & { rule: number; name: string };

/** One sender at the stranger gate (spec 1 section 6.7): its count of items, and the newest 3. */
/** A sender at the gate. `known_as`: the address of a person you know with the same name (issue #370). */
export type GateSender = { id: string; address: string; name: string | null; known_as?: string | null; count: number; items: Item[] };
export type GatePage = { count: number; senders: GateSender[]; next: string | null };
/** The sender row after an answer at the gate. */
export type GateAnswer = { id: string; address: string; name: string | null; kind: string; decided_by: string | null; declined: boolean };

export type Account = {
  id: number;
  address: string;
  provider: string;
  state: string | null;
  mirror_ready_at: string | null;
  sync: { kind: string; step: number | null } | null;
  /** `after_failure`: the sync before this one failed or stopped. An older engine leaves it out. */
  last_sync: { kind: string; started: string; ended: string | null; result: string | null; after_failure?: boolean } | null;
};

export type Status = {
  /** "observe" in release 1: Iris applies nothing. */
  mode?: string;
  accounts: Account[];
  messages: { total: number; fetched: number; understood: number; gone: number };
  /** False when no model is set: only the fixed rules read the mail. An older engine leaves it out. */
  model?: boolean;
  /** The live messages that wait for a model or a budget. Drafts and gone mail never count. An older engine leaves it out. */
  waits_model?: number;
  /** The time zone of defer dates and due dates (an IANA name, `iris zone`). Null or left out: the zone of the browser. */
  time_zone?: string | null;
  /** True when the owner set the time zone (`iris zone` or the Accounts page), false when it is the zone of the machine that runs Iris. */
  time_zone_set?: boolean;
  /** After model errors: the time of the next model call (RFC 3339), while it is later than now. An older engine leaves it out. */
  model_retry_at?: string;
};

/** A rule of policy.toml (spec 2 section 6.2). `when` holds only the conditions that the rule sets. */
export type Rule = {
  id: number;
  name: string;
  effect: "allow" | "review" | "deny" | "hide" | "bundle" | "notify";
  verbs: string[];
  when: Record<string, string | number>;
  by: "person" | "proposal";
  added: string;
  job: string | null;
  line: number;
};

export type Mode = "observe" | "apply";

/** A rule that Iris proposes. `rule` is the rule that an accept adds to the file; its `line` is 0. */
/** `kind`: `approvals` (an allow rule, R5-01) or `choices` (a hide rule from skips, mutes and dismisses, R5-02). */
export type Proposal = { id: number; kind?: "approvals" | "choices"; approvals: number; created_at: number; rule: Rule };

/** GET /api/policy: the active policy and the file on disk, compared to it. */
export type Policy = {
  mode: Mode;
  version: number | null;
  /** One more than the highest rule ID that a rule had: the ID of a removed rule is never used again. */
  next_id: number;
  rules: Rule[];
  /** The open rules that Iris proposes from your approvals (spec 2 section 6.6). */
  proposals?: Proposal[];
  error: { line: number; message: string } | null;
  file:
    | { state: "same" | "missing" }
    | { state: "invalid"; error: { line: number; message: string } }
    | { state: "changed"; sha256: string; text: string; mode: Mode; added: Rule[]; removed: Rule[]; changed: { before: Rule; after: Rule }[] };
};

// `next` goes back unchanged as `before=` (a form list) or `after=` (a brief section, an opaque cursor). Null on the last page.
export type Page = { items: Item[]; next: string | null; duplicates?: Record<string, string[]> };

export type Address = { name?: string | null; address?: string | null };

export type Summary = {
  id: string;
  thread: string;
  date: string | null;
  from: Address;
  subject: string | null;
  labels: string[];
};

/** POST /api/ask (card R5-09). `answer` has a mark `[n]` after each fact: the source with that `n`. */
export type AskSource = { n: number; quote: string | null; message: Summary };
export type AskAnswer = { answer: string | null; sources: AskSource[]; read: number; cost_micro_usd: number | null };

export type Message = Summary & {
  /** The To and Cc headers as text, or null. */
  to: string | null;
  cc: string | null;
  /** Clean text. Null when the engine removed the content of a gone mail (spec 1 section 4.5). */
  text: string | null;
  gone: boolean;
  hostile: string[];
  /** The kind of the From sender (spec 1 section 6.7). */
  sender_kind?: "person" | "service" | "stranger" | "hostile";
  /** The sender when only its look-alike domain makes it hostile: you can trust it (issue #49). */
  /** The trust offer: a sender that only the near-copy rule makes hostile (issue 49). */
  near_copy?: { sender: string; of: string } | null;
  /** The Undo of a trust: the sender row of this address that the person trusted (issue 1043). */
  trusted_sender?: string | null;
  /** The hostile marks that the person released (issues 369, 373, 374): Undo ends the release. */
  released?: string[];
  items: string[];
};

/** A changeset (spec 2 section 5.1): a request to change the mailbox, which a person reviews. */
export type Decision = {
  state: "proposed" | "in_review" | "approved" | "held" | "applied" | "failed" | "denied" | "expired" | "cancelled" | "replaced" | "undone";
  by?: "rule" | "person" | "policy" | "limit" | "engine" | "author" | "expiry";
  rule?: number;
  policy?: number;
  reason?: string;
  at?: string;
};

export type Action = {
  n: number;
  revision: number;
  verb: string;
  targets: { threads?: string[]; messages?: string[]; items?: string[]; drafts?: string[]; accounts?: string[]; ledger?: string[] };
  resolved: { messages: string[] };
  args: Record<string, unknown>;
  why: string | null;
  flags: string[];
  hash: string | null;
  decision: Decision | null;
};

/** A part of a draft text, with the draft flags that its words gave (card R4-07). `source` is the mail of copied text. */
export type Part = { text: string; flags?: string[]; source?: string };
/** A recipient of a draft, with the flag `new_recipient` if the owner never wrote to the address. */
export type Recipient = { address: string; flags?: string[] };
/** A file of a draft, as the review and the outbox show it. */
export type FileView = { name: string; type: string; size: number };
/** A file in the attachment folder of the engine (POST /api/attachments), with its name and type for a draft. */
export type Attachment = FileView & { sha256: string };
/**
 * GET /api/changesets/{id}/drafts: the draft of each `draft` and `send`
 * action that waits, as the review shows it. Each part of the text carries
 * the flags of the action that its words gave. An action whose text Iris
 * cannot read has `readable: false` and no text. A readable `send` of a
 * draft has `change`: how the write view changes the draft after the person
 * denies this send only, and `newer_mail` when a newer mail stops that
 * change (issue 1104). Absent from an older engine.
 */
export type DraftView = { revision: number; n: number; draft: string | null; thread: string | null } & (
  | {
      readable: true;
      to: Recipient[];
      cc: Recipient[];
      bcc?: Recipient[];
      attachments?: FileView[];
      subject: Part[];
      body: Part[];
      change?: "update" | "new" | null;
      newer_mail?: boolean;
    }
  | { readable: false }
);

export type Changeset = {
  id: string;
  revision: number;
  author: { kind: "person" | "agent" | "triage" | "local"; name?: string; session?: string; job?: string };
  /** Since open model 1.2: who withdrew the changeset (decision D16). Absent when it is not known. */
  withdrawn_by?: "author" | "person";
  why: string | null;
  created: string;
  submitted: string | null;
  state: "draft" | "open" | "closed" | "withdrawn";
  actions: Action[];
};

/**
 * GET /api/changesets/{id}/would: in observe mode, the decision that a rule of the active policy
 * would give to each proposed action in apply mode (decision D10). Empty in apply mode.
 */
export type Would = { revision: number; n: number; outcome: "approved" | "in_review" | "denied"; rule: number; policy: number; reason: string };

/** GET /api/handled: one action that an `allow` rule approved, with the reason. */
export type HandledAction = {
  changeset: string;
  revision: number;
  n: number;
  verb: string;
  args: Record<string, unknown>;
  /** The count of messages of the action. */
  messages: number;
  /** The state after the rule decision. A failed action can have changed some of its messages. */
  state: "approved" | "held" | "applied" | "failed" | "undone" | "denied";
  rule: number;
  policy: number;
  why: string | null;
  reason: string | null;
  at: string | null;
};

/** GET /api/ledger: one change that Iris made, with who decided it and if an undo can start (card R2-07). */
export type LedgerEntry = {
  id: string;
  verb: string;
  /** Only `label`, `until`, `date` and `field`. */
  args: Record<string, unknown>;
  state: "writing" | "applied" | "failed" | "undone";
  at: string;
  error: string | null;
  /** The messages of the entry, and the messages that Gmail confirmed. */
  messages: number;
  changed: number;
  /** The items of a state verb, and the title of the first item. */
  items: number;
  title: string | null;
  changeset: string;
  author: { kind: "person" | "agent" | "triage" | "local"; name: string | null; job: string | null; goal: string | null };
  decided_by: "rule" | "person" | null;
  rule: number | null;
  policy: number | null;
  reason: string | null;
  why: string | null;
  can_undo: boolean;
  undo_open: boolean;
  undone_by: string | null;
  undo_of: string | null;
  /** For an applied undo: the messages or items that it did not change (they changed again, or are gone). */
  skipped: number;
  /** For an undo: the targets that it tries to restore, also a gone message. 0 for each other entry. */
  tried: number;
  /** For an undo: true when the change that it undoes can be undone now. */
  retry: boolean;
  /** A send or a draft: the subject, the first `to` address and the count of all recipients. Null when Iris cannot read it. */
  mail?: { subject: string; to: string | null; recipients: number } | null;
  /** A draft: true when it changed a draft that exists (with `undo_of`: the undo of a change). An older engine leaves it out. */
  change?: boolean;
};

export type LedgerPage = { entries: LedgerEntry[]; next: string | null };

/** `count` is the count of all such actions, not only of this page. */
export type HandledPage = { count: number; actions: HandledAction[]; next: string | null };

/** One send in the outbox (GET /api/outbox): approved or held, with the words of its draft. */
export type OutboxSend = {
  draft: string;
  state: "approved" | "held";
  approved: string;
  /** The time that the send can leave. Null while sending is off. */
  leaves: string | null;
  flags: string[];
  readable: boolean;
  to?: string[];
  cc?: string[];
  bcc?: string[];
  attachments?: FileView[];
  subject?: string;
  /** The provider ("gmail" or "outlook") whose Bcc check keeps the send held, or null. An older engine leaves it out. */
  bcc_check?: string | null;
};
/** GET /api/outbox: the send switch and each send that waits. Only `iris sending on|off` changes the switch. */
export type Outbox = { sending: { on: boolean; since: string | null }; count: number; sends: OutboxSend[]; next: string | null };

/** The whole outbox: each page of GET /api/outbox, so no send that can leave is hidden. */
/** The words of a reply draft whose thread has a newer mail (draft change spec, section 5). */
export const NEWER_MAIL = "A new mail came into this thread. Write a new reply.";

/**
 * The start of #/write/d-N (issue 941): the form of a draft that can change,
 * under its thread for a reply. A draft that cannot change opens no form
 * (`closed`): the view says why and links to a new mail or a new reply.
 */
export type ChangeStart =
  | ({ change: DraftContent } & ({ thread: string; reply: ReplyStart } | { accounts: Status["accounts"] }))
  | { closed: DraftContent };

/** The session storage place of a form of the write view: a draft link (`d-N`), a reply (`t-N`) or `new`. */
export const writePlace = (of: string) => `iris.write.${of}`;

/** True while the write view keeps a write that the engine took from the place `place`: the write can still leave. */
export function keepsWrite(place: string): boolean {
  try {
    const v = JSON.parse(globalThis.sessionStorage?.getItem(`${place}.taken`) ?? "null");
    return !!v && typeof v.write === "string";
  } catch {
    return false;
  }
}

export async function loadChange(id: string): Promise<ChangeStart> {
  const change = await get<DraftContent>(`drafts/${encodeURIComponent(id)}`);
  // A write of this draft link that can still leave shows its state, also when its
  // own send stops the change of the draft now (issue 1098).
  const kept = keepsWrite(writePlace(id));
  if (!change.change && !kept) return { closed: change };
  if (change.thread) {
    try {
      return { thread: change.thread, reply: await get<ReplyStart>(`threads/${encodeURIComponent(change.thread)}/reply`), change };
    } catch (e) {
      // Only "no mail to reply to" (409) ends the reply start. Another error
      // (the engine is busy, no answer) shows, and the next open reads again.
      if (!kept || (e as { status?: number }).status !== 409) throw e;
      // The kept write needs no reply start (its parent can be in Trash now): it
      // shows with no conversation, and with no change, so no form follows it.
      // With no mail to reply to, the engine offers no change of the reply.
      return { accounts: [], change: { ...change, change: null } };
    }
  }
  return { accounts: (await get<Status>("status")).accounts, change };
}

export async function loadOutbox(): Promise<Outbox> {
  const first = await get<Outbox>("outbox");
  const sends = [...first.sends];
  let next = first.next;
  while (next) {
    const page = await get<Outbox>(`outbox?after=${encodeURIComponent(next)}`);
    sends.push(...page.sends);
    next = page.next;
  }
  return { ...first, sends, next: null };
}

/** GET /api/threads/{t}/draft: the ready draft of a thread (R3-09). `nudge` is true for a reminder to a late person (R3-07), with `to` and each late commitment that it names in `items`. */
export type ThreadDraft = { thread: string; based_on: string; nudge: boolean; to: string | null; items: string[]; body: string };

/** GET /api/items/{id}/summary: the summary of the long thread of an item that needs you (issue 478). `created` is the time that Iris wrote it. 404 when there is none, or when it is stale. */
export type ItemSummary = { text: string; based_on: string; created: string; model: string };

/** GET /api/threads/{t}/reply: the start values of a reply (write spec, section 5). */
export type ReplyStart = {
  account: string;
  parent: { id: string; from: Address; date: string | null; text: string | null };
  to: string[];
  cc: string[];
  subject: string;
  /** The subject of the parent as it is. The view shows it with no leading "Re:" as its title. */
  topic: string;
  /** The newest mails of the thread, oldest first; the last one is the parent. */
  conversation: {
    id: string;
    from: Address;
    date: string | null;
    to: string | null;
    you: boolean;
    hostile: boolean;
    text: string | null;
    cut: boolean;
  }[];
  /** The count of the older mails that `conversation` leaves out. */
  earlier: number;
};

/** GET /api/write/{cs}: the state of one press of Send (write spec, section 4). */
export type WriteState =
  | { state: "saving" }
  | { state: "waiting"; draft: string; sending: boolean; bcc_check?: string | null; leaves_at: string | null }
  | { state: "review"; draft: string; reason: string | null }
  | { state: "sent"; draft: string }
  | { state: "stopped"; reason: string; draft: string | null; change?: "update" | "new" | null; newer_mail?: boolean };

/**
 * GET /api/drafts/{id}: the content of a draft, for the form of "Change and
 * send" (draft change spec, section 5).
 */
export type DraftContent = {
  draft: string;
  account: string;
  thread: string | null;
  to: string[];
  cc: string[];
  bcc: string[];
  subject: string;
  body: string;
  attachments: Attachment[];
  /** How the next press of Send writes the draft, as `change` of a stopped write (issue 941). */
  change: "update" | "new" | null;
  /** Present (true) only when a newer mail in the thread of a reply stops the change. */
  newer_mail?: boolean;
};

/** A message summary, or only the ID of a message that is not in the mirror any more. */
export type Target = (Summary & { gone?: false }) | { id: string; gone: true };

/** A job of an agent (card R4-01, docs/spec/schema/job.schema.json). */
export type JobState = "working" | "waiting" | "needs_you" | "done";
export type JobWait = { thread?: string; until: string };
export type JobEvent = {
  at: string;
  state: JobState;
  by: "person" | "agent" | "engine";
  wait?: JobWait;
  question?: string;
  answer?: string;
  end?: "complete" | "cancel";
  why?: string;
};
export type Job = {
  id: string;
  goal: string;
  due?: string;
  created: string;
  updated: string;
  state: JobState;
  wait?: JobWait;
  question?: string;
  end?: "complete" | "cancel";
};
/**
 * `sessions`: the newest session of each working job of the page, by job ID, or null for a job with no session
 * (issue 791). An engine from before the field leaves it out: then the list does not know the session.
 */
export type JobPage = { jobs: Job[]; next: string | null; sessions?: Record<string, JobSession | null> };
/** A submitted changeset of a job: `actions` counts the actions of its newest revision in each state. */
export type JobChangeset = { id: string; at: string; state: string; why: string | null; actions: Record<string, number> };
/** One job (GET /api/jobs/{id}): the job with its events, its changesets, and the newest mail of each wait thread. */
/** The newest session of a job (issue #742). */
export type JobSession = {
  id: string;
  agent: string;
  state: "starting" | "running" | "ending" | "ended";
  end: "exit" | "stop" | "revoke" | "job_state" | "time_limit" | "engine" | "launch_failed" | "helper_failed" | null;
  exit: number | null;
  ended: string | null;
};
/** One thread or item in the scope of a job (decision D25): the mail that the agent reads. `id` is null for a row that the page cannot name. */
export type ScopeRef = { id: string | null; kind: "thread" | "item"; label: string | null; messages: number };
export type JobDetail = { job: Job & { events: JobEvent[] }; changesets: JobChangeset[]; threads: Record<string, Target>; scope?: ScopeRef[]; session?: JobSession | null };

export const SIGN_IN = "Sign in: run iris open in Terminal.";
export const SIGN_IN_FAILED = "This sign-in address was used, ended or is not valid. Run iris open in Terminal for a new address.";
export const NOT_COPIED = "Iris has not copied the original of this mail yet. Try again after the copy of the mailbox.";
export const SIGN_IN_RETRY = "The engine of Iris could not sign you in. Load this page again to try again.";
export const UNREACHABLE = "Iris cannot reach its engine. Start it with iris service install, then run iris open.";

/** The text that a view shows for an error: "" when App.svelte shows it already as the access state. */
export function own(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  return text === access.problem ? "" : text;
}

/**
 * The text that a view shows for a failed write. When the engine gave no answer,
 * the engine can have stored the write, so the text says that Iris does not know (issue 706).
 * `what` completes "Iris does not know if it ...". `otherwise` is for an error with no words.
 */
export function wrote(error: unknown, what: string, otherwise: string): string {
  if (error instanceof Error && error.message === UNREACHABLE) return `Iris does not know if it ${what}. Reload the page to see.`;
  return own(error) || otherwise;
}

/** Set the access state that App.svelte shows above each view, and throw it. */
// `status`: the HTTP status when the engine answered, so that a caller can tell a refusal from no answer.
function fail(problem: string, status?: number): never {
  access.problem = problem;
  throw Object.assign(new Error(problem), status ? { status } : {});
}

// Spec 2 section 8.6 and crates/iris-api/README.md: the API gives a token one
// time for the code of the sign-in address. The token stays in the storage of
// this origin only, not in a cookie, because a cookie goes to each port of a host.
type Store = Pick<Storage, "getItem" | "setItem" | "removeItem">;
const KEY = "iris.token";
const CODE = "iris.code";
function session(): Store | undefined {
  try {
    return globalThis.sessionStorage;
  } catch {
    return undefined;
  }
}
let token: string | null = null;
try {
  token = session()?.getItem(KEY) ?? null;
} catch {
  token = null;
}
let ready: Promise<void> = Promise.resolve();

// Keep `fn` from throwing when the browser blocks the storage.
function quiet<T>(fn: () => T): T | undefined {
  try {
    return fn();
  } catch {
    return undefined;
  }
}

/**
 * Trade the code in the address (`#code=...`) for the token. Does nothing with no code.
 * The code stays in the storage of this origin until the engine answers, so a
 * reload tries again after an exchange that did not reach the engine.
 */
export function signIn(
  loc: Pick<Location, "hash" | "pathname" | "search"> | undefined = globalThis.location,
  hist: Pick<History, "replaceState"> | undefined = globalThis.history,
  store: Store | undefined = session(),
): Promise<void> {
  const inAddress = loc?.hash.match(/^#code=([^&]+)/)?.[1];
  // Remove the code from the address bar and the history before the exchange.
  if (loc && inAddress) quiet(() => hist?.replaceState(null, "", `${loc.pathname}${loc.search}#/`));
  // A bad escape (for example `%`) gives undefined. This function must not throw: it runs at import.
  const fromAddress = inAddress ? quiet(() => decodeURIComponent(inAddress)) : undefined;
  if (inAddress && !fromAddress) return (ready = new Promise<void>(() => fail(SIGN_IN_FAILED)));
  const code = fromAddress ?? quiet(() => store?.getItem(CODE));
  if (!loc || !code) return Promise.resolve();
  if (fromAddress) quiet(() => store?.setItem(CODE, fromAddress));
  ready = (async () => {
    // No answer keeps the code for the next try (a reload).
    const response = await fetch("/api/session", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code }),
    }).catch(() => fail(UNREACHABLE));
    // The engine answered: it used the code or refused it. After a server error, keep it for a reload.
    if (response.status >= 500) fail(SIGN_IN_RETRY);
    quiet(() => store?.removeItem(CODE));
    const body = await response.json().catch(() => null);
    if (!response.ok || typeof body?.token !== "string") fail(SIGN_IN_FAILED);
    token = body.token;
    access.problem = "";
    // With no storage, the token works until the page closes.
    quiet(() => store?.setItem(KEY, body.token));
  })();
  return ready;
}
/**
 * For a sign-in address that the person opens in a tab that shows Iris already:
 * only the hash changes, so sign in, then load the page again with the token.
 * No reload after a failure, so a stopped engine does not make a reload loop.
 */
export async function followSignIn(
  loc: Pick<Location, "hash" | "pathname" | "search"> = globalThis.location,
  hist: Pick<History, "replaceState"> | undefined = globalThis.history,
  store: Store | undefined = session(),
  reload: () => void = () => globalThis.location.reload(),
): Promise<boolean> {
  if (!/^#code=/.test(loc.hash)) return false;
  const done = await signIn(loc, hist, store).then(() => true, () => false);
  if (done) reload();
  return done;
}

// Read the code before the first view reads the hash.
signIn().catch(() => {});

/** Number the requests of a view. A request applies its answer only while it is the newest. */
export function newest(): () => () => boolean {
  let last = 0;
  return () => {
    const mine = ++last;
    return () => mine === last;
  };
}

/** The browser limit of all keepalive bodies in flight is 64 KiB: keep a margin for others. */
export const KEEPALIVE_MAX = 60 * 1024;
export const keepable = (json: string) => new TextEncoder().encode(json).length <= KEEPALIVE_MAX;

// Each request of each view goes through here, so the access state covers every path.
async function call(path: string, accept: string, body?: unknown): Promise<Response> {
  // A failed sign-in does not stop a token that this page has from an earlier sign-in.
  const signInError = await ready.then(() => null, (e: Error) => e);
  const headers: Record<string, string> = { accept };
  // While the person is active, the local day: the engine counts it as a day of use, and a mail view as an open.
  if (active()) headers["x-iris-day"] = localDay();
  const sent = token;
  if (sent) headers.authorization = `Bearer ${sent}`;
  // A file (a Blob of any window) goes as its own bytes (POST /api/attachments).
  const file = typeof (body as Blob | undefined)?.arrayBuffer === "function" ? (body as Blob) : undefined;
  const json = body === undefined || file ? undefined : JSON.stringify(body);
  const init: RequestInit =
    file
      ? { method: "POST", headers: { ...headers, "content-type": "application/octet-stream" }, body: file }
      : json === undefined
        ? { headers }
        : // keepalive: a POST that starts as the page closes still reaches the engine (Act.svelte).
          // A browser refuses a keepalive body over 64 KiB, so a larger body (a long mail) goes without it.
          { method: "POST", keepalive: keepable(json), headers: { ...headers, "content-type": "application/json" }, body: json };
  const response = await fetch(`/api/${path}`, init).catch(() => fail(UNREACHABLE));
  if (response.status === 401) {
    // The engine does not know the token (for example after a restart of iris serve).
    // A sign-in during the request gave a new token: keep that one.
    if (sent === token) {
      token = null;
      quiet(() => session()?.removeItem(KEY));
    }
    if (token) return call(path, accept, body);
    fail(signInError?.message ?? SIGN_IN, 401);
  }
  access.problem = "";
  return response;
}

/** Get one JSON resource from the local API. Throws an Error with the API's error text. */
export async function get<T>(path: string): Promise<T> {
  if (path === "status") return (await statusRead())[0] as T;
  return getJson<T>(path);
}

async function getJson<T>(path: string): Promise<T> {
  const response = await call(path, "application/json");
  const body = await response.json().catch(() => null);
  // The status goes with the error, so that a caller can tell a 404 from other failures.
  if (!response.ok) throw Object.assign(new Error(body?.error ?? `The engine answered ${response.status}.`), { status: response.status });
  return body as T;
}

// The order of the status answers. Each request that answers with the status (a read, or a change
// such as a removal or a try now) takes a number at its start. An answer is the newest only if no
// answer of a request that started after it came first: requests in flight can answer out of order.
let statusStarted = 0;
let statusTaken = 0;

/** Take `s`, the answer of request `n`, if it is the newest. Gives true if it is. */
function takeStatus(n: number, s: Status): boolean {
  if (n <= statusTaken) return false;
  statusTaken = n;
  // Spec 1 section 4.7: the provider refused the sign-in of these accounts, so their sync stopped.
  const accounts = s.accounts ?? [];
  access.locked = accounts.filter((a) => a.state === "needs_sign_in").map(({ address, provider }) => ({ address, provider }));
  access.none = accounts.length === 0;
  access.accounts = accountNames(accounts);
  access.zone = s.time_zone ?? null;
  return true;
}

/** The answer of `POST /api/zone`: the time zone fields of the status. */
export type Zone = Pick<Status, "time_zone" | "time_zone_set">;

/**
 * Set the time zone of the owner (issue 891): an IANA name, or null for the zone of the machine that
 * runs Iris. The answer is the newest status of the zone: it takes a status number when it arrives,
 * so a status read that started before it cannot put the old zone back in `access.zone`.
 */
export async function postZone(zone: string | null): Promise<Zone> {
  const z = await post<Zone>("zone", { zone });
  statusTaken = ++statusStarted;
  access.zone = z.time_zone ?? null;
  return z;
}

async function statusRead(): Promise<[Status, boolean]> {
  const n = ++statusStarted;
  const s = await getJson<Status>("status");
  return [s, takeStatus(n, s)];
}

/** Read the status. Gives null if a newer status answered first: a view keeps the newer one. */
export async function readStatus(): Promise<Status | null> {
  const [s, newest] = await statusRead();
  return newest ? s : null;
}

/**
 * Send a change whose answer is the status (POST). Gives the status, or null if a newer status
 * answered first. A read that started during the change can show the state from before its commit,
 * so after a change, a view reads the status again.
 */
export async function postStatus(path: string, body: unknown): Promise<Status | null> {
  const n = ++statusStarted;
  const s = await post<Status>(path, body);
  return takeStatus(n, s) ? s : null;
}

/** Put one file into the attachment folder of the engine. The entry of a draft adds the name and the type. */
export async function upload(file: File): Promise<Attachment> {
  const v = await post<{ sha256: string; size: number }>("attachments", file);
  return { name: file.name, type: file.type.split(";")[0].trim() || "application/octet-stream", size: v.size, sha256: v.sha256 };
}

/** Send a policy change (POST with a JSON body). Throws an Error with the API's error text. */
export async function post<T>(path: string, body: unknown): Promise<T> {
  const response = await call(path, "application/json", body);
  const answer = await response.json().catch(() => null);
  // The body goes with the error too: a 409 of a start names the open job.
  if (!response.ok) throw Object.assign(new Error(answer?.error ?? `The engine answered ${response.status}.`), { status: response.status, body: answer });
  return answer as T;
}

function save(blob: Blob, name: string) {
  const url = URL.createObjectURL(blob);
  const link = Object.assign(document.createElement("a"), { href: url, download: name });
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** Download the original (RFC 5322) of a message. A link cannot send the token, so fetch it and save the blob. */
export async function download(id: string, keep: (blob: Blob, name: string) => void = save): Promise<void> {
  const response = await call(`messages/${encodeURIComponent(id)}/raw`, "message/rfc822");
  // The page hides the download of a removed mail, so a 404 here is a mail that the first copy has not fetched.
  if (response.status === 404) throw new Error(NOT_COPIED);
  if (!response.ok) {
    const body = await response.json().catch(() => null);
    throw new Error(body?.error ?? `The engine answered ${response.status}.`);
  }
  keep(await response.blob(), `${id}.eml`);
}

/**
 * The brief, with all of Signals. The API gives at most 200 items of a section and sorts
 * signals by expiry, so a live login code can be after the cut. Signals are few: load the
 * rest of the pages. Other sections show 7 lines and link their full list.
 */
export async function loadBrief(): Promise<Brief> {
  const brief = await get<Brief>("brief");
  if (brief.signals.items.length >= brief.signals.count) return brief;
  let items: Item[] = [];
  let next: string | null = null;
  do {
    const page: Page = await get<Page>(`brief/signals?limit=200${next ? `&after=${encodeURIComponent(next)}` : ""}`);
    items = addPage(items, page.items);
    next = page.next;
  } while (next);
  // A signal that came in between the calls makes the pages longer than the first count.
  return { ...brief, signals: { ...brief.signals, count: Math.max(brief.signals.count, items.length), items } };
}

/**
 * Your answer to a sender: `accept`, `decline` (the screen says "Ignore") or `reopen` (undo) at
 * the stranger gate, or `trust` for a sender that only its look-alike domain makes hostile.
 */
/** The hostile marks that the person can release: the checks of Iris. `spam`, `phishing` and a hostile sender stay. */
export const RELEASABLE = ["dmarc", "spoof", "link"];

/** Release a mail from quarantine, or end the release (`unrelease`). The answer is the mail. */
export function releaseMessage(id: string, verb: "release" | "unrelease"): Promise<Message> {
  return post<Message>(`messages/${encodeURIComponent(id)}/${verb}`, {});
}

export function answerSender(id: string, answer: "accept" | "decline" | "reopen" | "trust"): Promise<GateAnswer> {
  return post<GateAnswer>(`senders/${encodeURIComponent(id)}/${answer}`, {});
}
