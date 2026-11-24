import { access } from "./access.svelte.ts";
import { today } from "./act";
import type { Account, Brief, Item, MatterForm, Money, Party, Section, Status } from "./api";

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
export function when(value: string | null | undefined): string {
  if (!value) return "";
  const date = toDate(value);
  if (Number.isNaN(date.getTime())) return value;
  return isDay(value)
    ? date.toLocaleDateString(undefined, { dateStyle: "medium" })
    : date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

const relative = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
const DAY = 86_400_000;

/**
 * Whole days from the day of `now` to the day of `value`, in the time zone
 * of the owner (`zone`, an IANA name; by default the one of the engine
 * status), so that "due tomorrow" and Due soon agree with the engine
 * (decision D1). A date (YYYY-MM-DD) is that day.
 */
export function daysFrom(value: string, now = new Date(), zone: string | null | undefined = access.zone): number {
  const day = isDay(value) ? value : today(toDate(value), zone);
  return Math.round((Date.parse(day) - Date.parse(today(now, zone))) / DAY);
}

export type Due = { text: string; tone: "late" | "soon" | "" };

/** The due date of a commitment as words: "due tomorrow", "was due 2 days ago". */
export function due(item: Item, now = new Date(), zone: string | null | undefined = access.zone): Due | null {
  const value = item.due?.date ?? item.due?.at;
  if (!value || Number.isNaN(toDate(value).getTime())) return null;
  const days = daysFrom(value, now, zone);
  const words = relative.format(days, "day");
  // A due time earlier today has passed, although its day is today.
  const at = item.due?.at ? Date.parse(item.due.at) : NaN;
  if (days < 0 || at <= now.getTime()) return { text: `was due ${words}`, tone: "late" };
  return { text: `due ${words}`, tone: days <= 2 ? "soon" : "" };
}

/** The commitments of `items` that are due in 2 days or less, or late, on the day of `now` in the time zone of the owner. */
export function dueSoon(items: Item[], now = new Date(), zone: string | null | undefined = access.zone): Item[] {
  return items.filter((item) => !!due(item, now, zone)?.tone);
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

/** The items whose expiry is later than `now`, or that have no expiry (spec 1 section 8, Signals). */
export function unexpired(items: Item[], now: Date): Item[] {
  return items.filter((i) => !i.expires || Date.parse(i.expires) > now.getTime());
}

/** The first expiry after `now` among the items, or undefined. A view wakes at it. */
export function nextExpiry(items: Item[], now: Date): string | undefined {
  return unexpired(items, now)
    .map((i) => i.expires)
    .filter((e): e is string => !!e)
    .sort((a, b) => Date.parse(a) - Date.parse(b))[0];
}

/**
 * The Signals of the brief at `now` (spec 1 section 8): a signal leaves at its expiry, and a live
 * login code shows at the top, not in the rows. `next` is the first expiry still to come.
 */
export function signals(section: Section, now: Date) {
  const live = unexpired(section.items, now);
  const codes = live.filter((i) => liveCode(i, now));
  const next = nextExpiry(live, now);
  return { codes, rows: live.filter((i) => !codes.includes(i)), count: section.count - (section.items.length - live.length), next };
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
      // The latest sender can be you: then name another participant.
      return item.latest?.from && item.latest.from.kind !== "self"
        ? item.latest.from
        : item.participants?.find((p) => p.kind !== "self");
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
  if (Array.isArray(value)) return value.map(field).filter(Boolean).join(", ");
  // A party (spec 1 section 6.4) shows as a person: its name, else its address.
  if (typeof value === "object" && ("kind" in value || "name" in value)) return party(value as Party);
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
export const count = (n: number, one: string, many: string) => `${WORDS[n] ?? n.toLocaleString("en-US")} ${n === 1 ? one : many}`;
export const capital = (s: string) => s[0].toUpperCase() + s.slice(1);

/** The members of a matter by form: "Two records, one conversation". */
export function matterCounts(counts: Record<MatterForm, number>): string {
  const forms: MatterForm[] = ["record", "conversation", "commitment"];
  const text = forms.filter((f) => counts[f] > 0).map((f) => count(counts[f], f, `${f}s`)).join(", ");
  return text ? capital(text) : "No items now";
}

/** The start of the next local day, as an ISO date-time. */
export function nextDay(now = new Date(), zone: string | null | undefined = access.zone): string {
  const browser = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1).getTime();
  let owner = browser;
  try {
    if (zone) {
      const parts = new Intl.DateTimeFormat("en-US", { timeZone: zone, hourCycle: "h23", hour: "numeric", minute: "numeric", second: "numeric" }).formatToParts(now);
      const part = (type: string) => Number(parts.find((p) => p.type === type)?.value ?? 0);
      const elapsed = ((part("hour") * 60 + part("minute")) * 60 + part("second")) * 1000 + now.getMilliseconds();
      // shortcut: a day with a clock change is 23 or 25 hours, so this can be one hour off; the next call corrects it.
      owner = now.getTime() - elapsed + DAY;
    }
  } catch {
    // An unknown zone: the midnight of the browser.
  }
  return new Date(Math.min(browser, owner)).toISOString();
}

export function greeting(now = new Date()): string {
  const h = now.getHours();
  return h < 5 ? "Good evening." : h < 12 ? "Good morning." : h < 18 ? "Good afternoon." : "Good evening.";
}

/** Copy a text. Returns false when the browser refuses, so the view can say so. */
export async function copyText(text: string, clipboard: Pick<Clipboard, "writeText"> | undefined = globalThis.navigator?.clipboard): Promise<boolean> {
  try {
    if (!clipboard) return false;
    await clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** The first lines of the brief: what needs you, in words. */
export const NOTHING = "Nothing needs you now.";

export function summary(brief: Brief): string {
  // Use the section counts, not the items: the API gives at most 200 items of a section.
  const talks = brief.conversations.count;
  const parts = [];
  const review = brief.review?.actions ?? 0;
  if (review) parts.push(`${capital(count(review, "change waits", "changes wait"))} for your review.`);
  if (brief.you_owe.count) parts.push(`You owe ${count(brief.you_owe.count, "thing", "things")}.`);
  if (brief.owed_to_you.count) parts.push(`${capital(count(brief.owed_to_you.count, "thing is", "things are"))} owed to you.`);
  if (talks) parts.push(`${capital(count(talks, "conversation waits", "conversations wait"))} for you.`);
  // An API with the stranger gate counts the senders that wait for an answer.
  const gate = brief.gate?.count;
  const odd = brief.strangers.count;
  if (gate) parts.push(`${capital(count(gate, "sender that you do not know waits", "senders that you do not know wait"))} for an answer.`);
  else if (odd && gate === undefined) parts.push(`${capital(count(odd, "item is from a sender", "items are from senders"))} you do not know.`);
  return parts.length ? parts.join(" ") : NOTHING;
}

/** The accounts whose first sync is not complete (spec 1 section 4.2): the brief then covers only the mail fetched so far. */
export function unfinished(status: Status | null): Account[] {
  return status ? status.accounts.filter((a) => !a.mirror_ready_at) : [];
}

/** The accounts that still copy in their first sync. A refused sign-in stops the sync (spec 1 section 4.7). */
export function copying(status: Status | null): Account[] {
  return unfinished(status).filter((a) => a.state !== "needs_sign_in");
}

/** The API counts the messages of all accounts together: say so when there are two or more. */
export function across(status: Status | null): string {
  return status && status.accounts.length > 1 ? " from all accounts" : "";
}

/** One shell argument: as it is if it has only safe characters, else in single quotes. */
export function shellArg(s: string): string {
  return /^[A-Za-z0-9@._+-]+$/.test(s) ? s : `'${s.replaceAll("'", `'\\''`)}'`;
}

/**
 * Who refused the sign-in of an account, and the command that signs it in
 * again on the machine that runs Iris. The command differs for each provider.
 */
export function signIn(a: Pick<Account, "address" | "provider">): { who: string; command: string; detail: string } {
  if (a.provider === "gmail") return { who: "Google", command: "iris account add client.json", detail: "with the client file of this account" };
  // With no --host, the engine uses the saved server, port and user of the account.
  // `--` ends the options, so an address that starts with a dash stays an address (issue 694).
  if (a.provider === "imap") return { who: "The mail server", command: `iris account add-imap ${a.address.startsWith("-") ? "-- " : ""}${shellArg(a.address)}`, detail: "and give the new password" };
  if (a.provider === "outlook") return { who: "Microsoft", command: "iris account add-outlook <client ID>", detail: "with the application (client) ID of your app registration. For a single-tenant registration, also add --tenant with its directory (tenant) ID" };
  return { who: "The provider", command: "iris account --help", detail: "to see the sign-in command of this provider" };
}

/**
 * A short name for each account, by its `a-` ID, so a line can say which mailbox
 * an item came from (card R6-02). The first part that tells the accounts apart:
 * the name of the domain ("gmail"), the part before "@", or the whole address.
 * With one account, no line needs a name: the map is empty.
 */
export function accountNames(accounts: Pick<Account, "id" | "address">[]): Record<string, string> {
  if (accounts.length < 2) return {};
  const parts = [
    (a: string) => a.split("@")[1]?.split(".")[0] ?? a,
    (a: string) => a.split("@")[0],
    (a: string) => a,
  ];
  const lower = accounts.map((a) => a.address.toLowerCase());
  const pick = parts.find((p) => new Set(lower.map(p)).size === lower.length) ?? parts[2];
  return Object.fromEntries(accounts.map((a, i) => [`a-${a.id}`, pick(lower[i])]));
}

/** The home view: the setup flow until there is something to show, then the brief. */
export function home(status: Status, view: "home" | "brief" = "home"): "setup" | "brief" {
  if (status.accounts.length === 0) return "setup";
  // The Brief link of the header opens the brief also while the first sync has no understood mail.
  if (view === "brief") return "brief";
  const ready = status.accounts.some((a) => a.mirror_ready_at);
  // Spec 1 section 4.2: the first brief shows during the first sync, from the mail fetched so far.
  return ready || status.messages.understood > 0 ? "brief" : "setup";
}

/** Add a next page to a list. Keyset paging can repeat an item, and a keyed list must not show an ID twice. */
export function addPage(items: Item[], more: Item[]): Item[] {
  const seen = new Set(items.map((i) => i.id));
  return [...items, ...more.filter((i) => !seen.has(i.id))];
}

/**
 * True when no model is set and the understanding step marked live mail as waiting for one (issue 740):
 * only the fixed rules read mail. Drafts and gone mail never count (the engine gives `waits_model`).
 */
export const needsModel = (status: Status | null): boolean => status?.model === false && (status.waits_model ?? 0) > 0;

/** The size of a file in short words: "820 B", "12 KB", "2.4 MB". */
export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/** The files of a mail in one line: "q4.pdf (12 KB), notes.txt (820 B)". */
export const fileLine = (files: { name: string; size: number }[]) => files.map((f) => `${f.name} (${bytes(f.size)})`).join(", ");
