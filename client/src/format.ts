import type { Brief, Item, Money, Party, Status } from "./api";

export function party(p: Party | undefined): string {
  if (!p) return "";
  if (p.kind === "self") return "You";
  return p.name || p.address || "Unknown sender";
}

/** One or two letters for the round mark of a party. */
export function initials(p: Party | undefined): string {
  const name = party(p);
  if (!p || p.kind === "self") return "Y";
  const words = (p.name || name.split("@")[0]).split(/[\s._-]+/).filter(Boolean);
  return words.slice(0, 2).map((w) => w[0].toUpperCase()).join("") || "?";
}

/** A key for a party that two senders with the same name do not share. */
export function partyKey(p: Party | undefined): string {
  return p?.sender ?? p?.address ?? party(p);
}

export function money(m: Money | undefined): string {
  return m ? `${m.value} ${m.currency}` : "";
}

const isDay = (value: string) => /^\d{4}-\d{2}-\d{2}$/.test(value);
const toDate = (value: string) => new Date(isDay(value) ? `${value}T00:00:00` : value);

/** A date (YYYY-MM-DD) or a date-time, in the language of the browser. */
export function when(value: string | undefined): string {
  if (!value) return "";
  const date = toDate(value);
  if (Number.isNaN(date.getTime())) return value;
  return isDay(value)
    ? date.toLocaleDateString(undefined, { dateStyle: "medium" })
    : date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

const relative = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
const DAY = 86_400_000;

/** Whole days from the local day of `now` to the local day of `value`. */
export function daysFrom(value: string, now = new Date()): number {
  const d = toDate(value);
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  return Math.round((start(d) - start(now)) / DAY);
}

export type Due = { text: string; tone: "late" | "soon" | "" };

/** The due date of a commitment as words: "due tomorrow", "was due 2 days ago". */
export function due(item: Item, now = new Date()): Due | null {
  const value = item.due?.date ?? item.due?.at;
  if (!value || Number.isNaN(toDate(value).getTime())) return null;
  const days = daysFrom(value, now);
  const words = relative.format(days, "day");
  if (days < 0) return { text: `was due ${words}`, tone: "late" };
  return { text: `due ${words}`, tone: days <= 2 ? "soon" : "" };
}

/** Time left until a date-time, as words: "in 9 minutes". Null after the time. */
export function timeLeft(value: string | undefined, now = new Date()): string | null {
  if (!value) return null;
  const ms = Date.parse(value) - now.getTime();
  if (!(ms > 0)) return null;
  const minutes = Math.ceil(ms / 60_000);
  if (minutes < 60) return relative.format(minutes, "minute");
  const hours = Math.round(minutes / 60);
  return hours < 48 ? relative.format(hours, "hour") : relative.format(Math.round(hours / 24), "day");
}

/** True when a signal is a login code that is still good at `now` (spec 1 section 6.6). */
export function liveCode(item: Item, now = new Date()): boolean {
  return item.form === "signal" && !!item.code && item.status === "active" && timeLeft(item.expires, now) !== null;
}

/** The origin source of an item: the message that made it (spec 1 section 6.1). */
export function origin(item: Item): string {
  return (item.sources.find((s) => s.role === "origin") ?? item.sources[0]).message;
}

/** The party that a line shows for an item. */
export function other(item: Item): Party | undefined {
  switch (item.form) {
    case "commitment":
      return item.direction === "you_owe" ? item.counterparty : item.owner;
    case "conversation":
      return item.latest?.from ?? item.participants?.find((p) => p.kind !== "self");
    case "record":
    case "signal":
      return item.issuer;
    case "feed":
      return item.publisher;
  }
}

/** Show a value of a record field: money, a list of entries, or text. */
export function field(value: unknown): string {
  if (value == null) return "";
  if (typeof value === "object" && "value" in value && "currency" in value) return money(value as Money);
  if (Array.isArray(value)) return value.map((v) => (typeof v === "object" && v && "name" in v ? String(v.name) : field(v))).join(", ");
  if (typeof value === "object") return Object.values(value).map(field).filter(Boolean).join(", ");
  const text = String(value);
  return /^\d{4}-\d{2}-\d{2}(T|$)/.test(text) ? when(text) : text;
}

/** The fields of each record type (spec 1 section 6.4), in table order. */
export const RECORD_FIELDS: Record<string, string[]> = {
  receipt: ["vendor", "amount", "date", "order", "items"],
  booking: ["kind", "reference", "start", "end", "location"],
  statement: ["issuer_name", "period_start", "period_end", "balance", "due_date"],
  ticket: ["event", "start", "venue", "seat"],
  contract: ["name", "parties", "effective", "expires"],
  event: ["name", "start", "end", "location"],
  other: ["summary"],
};

const WORDS = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];
const count = (n: number, one: string, many: string) => `${WORDS[n] ?? n.toLocaleString("en-US")} ${n === 1 ? one : many}`;
const capital = (s: string) => s[0].toUpperCase() + s.slice(1);

export function greeting(now = new Date()): string {
  const h = now.getHours();
  return h < 5 ? "Good evening." : h < 12 ? "Good morning." : h < 18 ? "Good afternoon." : "Good evening.";
}

/** The first lines of the brief: what needs you, in words. */
export function summary(brief: Brief): string {
  const turns = brief.conversations.items.filter((c) => c.your_turn).length;
  const parts = [];
  if (brief.you_owe.count) parts.push(`You owe ${count(brief.you_owe.count, "thing", "things")}.`);
  if (brief.owed_to_you.count) parts.push(`${capital(count(brief.owed_to_you.count, "thing is", "things are"))} owed to you.`);
  if (turns) parts.push(`${capital(count(turns, "conversation waits", "conversations wait"))} for your reply.`);
  return parts.length ? parts.join(" ") : "Nothing needs you now.";
}

/** The home view: the setup flow until there is something to show, then the brief. */
export function home(status: Status): "setup" | "brief" {
  if (status.accounts.length === 0) return "setup";
  const ready = status.accounts.some((a) => a.mirror_ready_at);
  // Spec 1 section 4.2: the first brief shows during the first sync, from the mail fetched so far.
  return ready || status.messages.understood > 0 ? "brief" : "setup";
}
