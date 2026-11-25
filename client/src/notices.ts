// Notices by rule (card R5-08): one notice for each new mail that a `notify` rule matched.
// Job notices (docs/specs/2026-11-25-job-notices.md): one notice for each move of a job to needs you.
import { get, post, type Summary } from "./api";

export type Notice = {
  id: string;
  /** The rule. Null for a job notice. */
  rule: number | null;
  /** The `j-` ID of the job of a job notice. Null or absent for a notice by rule. */
  job?: string | null;
  /** The name of the rule, or `Job due` or `Job question`. */
  rule_name: string;
  /** The sender, or the job ID, one line. */
  title: string;
  /** The subject, or the goal of the job, one line. */
  text: string;
  created: string;
  /** The mail. Null for a job notice, and when the mail left the mailbox. */
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
