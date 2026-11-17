// Watches (card R5-07): a search text that gives one notice when a new mail matches.
import { get, post, type Summary } from "./api";
import { capital, count } from "./format";

export type Watch = {
  id: string;
  text: string;
  created: string;
  state: "waits" | "arrived";
  arrived: string | null;
  /** The first new mail that matched. Null while the watch waits, and after the mail is gone. */
  message: Summary | null;
};

/** The watches, newest first. An engine from before R5-07 has no route (404): no watches. */
export async function loadWatches(): Promise<Watch[]> {
  try {
    return await get<Watch[]>("watches");
  } catch (e) {
    if ((e as { status?: number }).status === 404) return [];
    throw e;
  }
}

export const addWatch = (text: string) => post<Watch>("watches", { text });
export const removeWatch = (id: string) => post<{ removed: string }>(`watches/${encodeURIComponent(id)}/remove`, {});

/** The first sentence of the brief when one or more watched mails came. */
export function arrivedLine(arrived: Watch[]): string {
  if (!arrived.length) return "";
  return arrived.length === 1 ? "The mail that you watched for came." : `${capital(count(arrived.length, "mail", "mails"))} that you watched for came.`;
}

/** The local day of a date-time, for example "Nov 16, 2026". The API gives UTC, so the day can differ from its first 10 characters. */
export function day(value: string): string {
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? value : d.toLocaleDateString(undefined, { dateStyle: "medium" });
}
