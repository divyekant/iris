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
  sync: { kind: string; step: number } | null;
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

export type Summary = {
  id: string;
  thread: string;
  date: string;
  from: { name?: string; address: string };
  subject: string;
  labels: string[];
};

export type Message = Summary & {
  to: { name?: string; address: string }[];
  cc: { name?: string; address: string }[];
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

// Spec 2 section 8.6 and crates/iris-api/README.md: the API gives a token one
// time for the code of the sign-in address. The token stays in the storage of
// this origin only, not in a cookie, because a cookie goes to each port of a host.
type Store = Pick<Storage, "getItem" | "setItem" | "removeItem">;
const KEY = "iris.token";
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

/** Trade the code in the address (`#code=...`) for the token. Does nothing with no code. */
export function signIn(
  loc: Pick<Location, "hash" | "pathname" | "search"> | undefined = globalThis.location,
  hist: Pick<History, "replaceState"> | undefined = globalThis.history,
  store: Store | undefined = session(),
): Promise<void> {
  const code = loc?.hash.match(/^#code=([^&]+)/)?.[1];
  if (!loc || !code) return Promise.resolve();
  // Remove the code from the address bar and the history before the exchange.
  hist?.replaceState(null, "", `${loc.pathname}${loc.search}#/`);
  ready = (async () => {
    const response = await fetch("/api/session", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code: decodeURIComponent(code) }),
    });
    const body = await response.json().catch(() => null);
    if (!response.ok || typeof body?.token !== "string") throw new Error(SIGN_IN_FAILED);
    token = body.token;
    try {
      store?.setItem(KEY, body.token);
    } catch {
      // No storage: the token works until the page closes.
    }
  })();
  return ready;
}
// Read the code before the first view reads the hash.
signIn().catch(() => {});

async function call(path: string, accept: string): Promise<Response> {
  await ready;
  const headers: Record<string, string> = { accept };
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fetch(`/api/${path}`, { headers });
  if (response.status === 401) throw new Error(SIGN_IN);
  return response;
}

/** Get one JSON resource from the local API. Throws an Error with the API's error text. */
export async function get<T>(path: string): Promise<T> {
  const response = await call(path, "application/json");
  const body = await response.json().catch(() => null);
  if (!response.ok) throw new Error(body?.error ?? `The engine answered ${response.status}.`);
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
  if (!response.ok) {
    const body = await response.json().catch(() => null);
    throw new Error(body?.error ?? `The engine answered ${response.status}.`);
  }
  keep(await response.blob(), `${id}.eml`);
}
