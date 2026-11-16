import type { Item, Money, Party } from "./api";

export function party(p: Party | undefined): string {
  if (!p) return "";
  if (p.kind === "self") return "You";
  return p.name || p.address || "Unknown sender";
}

export function money(m: Money | undefined): string {
  return m ? `${m.value} ${m.currency}` : "";
}

/** A date (YYYY-MM-DD) or a date-time, in the language of the browser. */
export function when(value: string | undefined): string {
  if (!value) return "";
  const date = new Date(value.length === 10 ? `${value}T00:00:00` : value);
  if (Number.isNaN(date.getTime())) return value;
  return value.length === 10
    ? date.toLocaleDateString(undefined, { dateStyle: "medium" })
    : date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** The origin source of an item: the message that made it (spec 1 section 6.1). */
export function origin(item: Item): string {
  return (item.sources.find((s) => s.role === "origin") ?? item.sources[0]).message;
}

/** The other party of an item, as one short text for a line. */
export function who(item: Item): string {
  switch (item.form) {
    case "commitment":
      return party(item.direction === "you_owe" ? item.counterparty : item.owner);
    case "conversation":
      return party(item.latest?.from);
    case "record":
    case "signal":
      return party(item.issuer);
    case "feed":
      return party(item.publisher);
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
