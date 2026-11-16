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

// `next` is the ID before which the next page starts, or an offset for a section of the brief.
export type Page = { items: Item[]; next: string | number | null; duplicates?: Record<string, string[]> };

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
  text: string;
  gone: boolean;
  hostile: string[];
  items: string[];
};

/** Get one JSON resource from the local API. Throws an Error with the API's error text. */
export async function get<T>(path: string): Promise<T> {
  const response = await fetch(`/api/${path}`, { headers: { accept: "application/json" } });
  const body = await response.json().catch(() => null);
  if (!response.ok) throw new Error(body?.error ?? `The engine answered ${response.status}.`);
  return body as T;
}
