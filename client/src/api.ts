import { access } from "./access.svelte.ts";
import { addPage } from "./format";
// The client reads the local API of the engine (crates/iris-api/README.md).
// The form documents follow docs/spec/schema. The client only reads.

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
  confidence?: number;
  // Fields of each form. They are optional here, so one type holds all forms.
  direction?: "you_owe" | "owed_to_you";
  kind?: string;
  owner?: Party;
  counterparty?: Party;
  due?: { date?: string; at?: string; strict?: boolean };
  amount?: Money;
  participants?: Party[];
  latest?: { at: string; from: Party; snippet: string };
  messages?: number;
  new?: number;
  your_turn?: boolean;
  summary?: string;
  type?: string;
  issuer?: Party;
  fields?: Record<string, unknown>;
  publisher?: Party;
  list?: string;
  published?: string;
  code?: string;
  expires?: string;
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
  /** The release 1 triage proposal (spec 1 section 8): computed, not stored, nothing applied. */
  proposal?: { count: number; verbs: Record<string, number> } | null;
};

export type Account = {
  id: number;
  address: string;
  provider: string;
  state: string | null;
  mirror_ready_at: string | null;
  sync: { kind: string; step: number | null } | null;
  last_sync: { kind: string; started: string; ended: string | null; result: string | null } | null;
};

export type Status = {
  /** "observe" in release 1: Iris applies nothing. */
  mode?: string;
  accounts: Account[];
  messages: { total: number; fetched: number; understood: number; gone: number };
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
  items: string[];
};

export const SIGN_IN = "Open the sign-in address that iris serve printed.";
export const SIGN_IN_FAILED = "This sign-in address was used or is not valid. Start iris serve again, then open the new address.";
export const NOT_COPIED = "Iris has not copied the original of this mail yet. Try again after the copy of the mailbox.";
export const SIGN_IN_RETRY = "The engine of Iris could not sign you in. Load this page again to try again.";
export const UNREACHABLE = "Iris cannot reach its engine. Start it with iris serve, then open the sign-in address that it prints.";

/** The text that a view shows for an error: "" when App.svelte shows it already as the access state. */
export function own(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  return text === access.problem ? "" : text;
}

/** Set the access state that App.svelte shows above each view, and throw it. */
function fail(problem: string): never {
  access.problem = problem;
  throw new Error(problem);
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

// Each request of each view goes through here, so the access state covers every path.
async function call(path: string, accept: string): Promise<Response> {
  // A failed sign-in does not stop a token that this page has from an earlier sign-in.
  const signInError = await ready.then(() => null, (e: Error) => e);
  const headers: Record<string, string> = { accept };
  const sent = token;
  if (sent) headers.authorization = `Bearer ${sent}`;
  const response = await fetch(`/api/${path}`, { headers }).catch(() => fail(UNREACHABLE));
  if (response.status === 401) {
    // The engine does not know the token (for example after a restart of iris serve).
    // A sign-in during the request gave a new token: keep that one.
    if (sent === token) {
      token = null;
      quiet(() => session()?.removeItem(KEY));
    }
    if (token) return call(path, accept);
    fail(signInError?.message ?? SIGN_IN);
  }
  access.problem = "";
  return response;
}

/** Get one JSON resource from the local API. Throws an Error with the API's error text. */
export async function get<T>(path: string): Promise<T> {
  const response = await call(path, "application/json");
  const body = await response.json().catch(() => null);
  if (!response.ok) throw new Error(body?.error ?? `The engine answered ${response.status}.`);
  // Spec 1 section 4.7: Google refused the refresh token of these accounts, so their sync stopped.
  if (path === "status") {
    const accounts = (body as Status).accounts ?? [];
    access.locked = accounts.filter((a) => a.state === "needs_sign_in").map((a) => a.address);
    access.none = accounts.length === 0;
  }
  return body as T;
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
