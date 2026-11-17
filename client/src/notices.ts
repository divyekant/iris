// Notices by rule (card R5-08): one notice for each new mail that a `notify` rule matched.
import { get, post, type Summary } from "./api";

export type Notice = {
  id: string;
  rule: number;
  rule_name: string;
  /** The sender, one line. */
  title: string;
  /** The subject, one line. */
  text: string;
  created: string;
  /** The mail. Null when it left the mailbox. */
  message: Summary | null;
};

/** The notices that are not done, newest first. An engine from before R5-08 has no route (404): no notices. */
export async function loadNotices(): Promise<Notice[]> {
  try {
    return await get<Notice[]>("notices");
  } catch (e) {
    if ((e as { status?: number }).status === 404) return [];
    throw e;
  }
}

export const doneNotice = (id: string) => post<{ done: string }>(`notices/${encodeURIComponent(id)}/done`, {});

/** The engine adds notices each 10 seconds. */
export const NOTICE_POLL = 10_000;
