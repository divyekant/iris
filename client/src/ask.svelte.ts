// The questions of the Ask view (card R5-09) and their answers. The thread
// lives in this module, so it stays while the person opens a source mail and
// comes back. It goes when the page closes or loads again.
import { own, post, type AskAnswer, type AskSource } from "./api";

export type Turn = { question: string; answer?: AskAnswer; error?: string };

/** `draft` is the text of the question box. It lives here too, so a failed
 * question comes back to the box also after the view closed and opened. */
export const thread = $state({ turns: [] as Turn[], busy: false, draft: "" });

/** Ask one question. Gives false if a question waits for its answer, or the question is empty. */
export async function ask(question: string): Promise<boolean> {
  question = question.trim();
  if (!question || thread.busy) return false;
  thread.busy = true;
  thread.turns.push({ question });
  const turn = thread.turns[thread.turns.length - 1];
  try {
    turn.answer = await post<AskAnswer>("ask", { question });
  } catch (e) {
    // own() gives "" when App.svelte shows the error already as the access state.
    turn.error = own(e) || "Iris could not answer. The notice above says why.";
    // Give the question back, so the person can send it again. Keep new text.
    if (!thread.draft) thread.draft = question;
  } finally {
    thread.busy = false;
  }
  return true;
}

export type Part = { text: string } | { source: AskSource };

/** The answer as text and source marks. A mark is `[n]` with n from 1, as the engine writes it
 * (no leading 0). A mark with no source `n` stays as text. */
export function parts(answer: string, sources: AskSource[]): Part[] {
  const out: Part[] = [];
  let last = 0;
  for (const m of answer.matchAll(/\[([1-9]\d{0,2})\]/g)) {
    const source = sources.find((s) => s.n === Number(m[1]));
    if (!source) continue;
    // The mark sits close to its fact: no space before it.
    const text = answer.slice(last, m.index).trimEnd();
    if (text) out.push({ text });
    out.push({ source });
    last = m.index + m[0].length;
  }
  if (last < answer.length) out.push({ text: answer.slice(last) });
  return out;
}

/** The cost of an answer in words, or "" for no cost (no call, a local model, or a price of 0). */
export function cost(micro: number | null): string {
  if (micro === null || micro <= 0) return "";
  // Cents, rounded to one decimal.
  const n = Math.round(micro / 1_000) / 10;
  if (n >= 100) return `$${(micro / 1_000_000).toFixed(2)}`;
  if (n < 0.1) return "less than 0.1 cents";
  return n === 1 ? "1 cent" : `${n} cents`;
}
