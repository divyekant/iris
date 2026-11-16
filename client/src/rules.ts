// Words for the rules of policy.toml (spec 2 section 6.2), so that the policy
// view reads as sentences, not as fields.
import type { Mode, Rule } from "./api";

const VERBS: Record<string, string> = {
  archive: "archive",
  inbox: "move to the inbox",
  label: "add a label",
  unlabel: "remove a label",
  read: "mark as read",
  unread: "mark as unread",
  trash: "move to the trash",
  close: "close",
  defer: "defer",
  decline: "decline",
  correct: "correct",
};

const FORMS: Record<string, string> = {
  commitment: "Commitments",
  conversation: "Conversations",
  record: "Records",
  feed: "Feed",
  signal: "Signals",
};

const KINDS: Record<string, string> = {
  self: "From you",
  person: "From people",
  service: "From services",
  stranger: "From strangers",
  hostile: "Hostile mail",
};

const PLURAL: Record<string, string> = { reply: "Replies", other: "Other records" };

/** "A", "A and B", "A, B and C". */
export function list(words: string[]): string {
  return words.length < 2 ? (words[0] ?? "") : `${words.slice(0, -1).join(", ")} and ${words[words.length - 1]}`;
}

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** What an action rule lets Iris do, as one sentence: "Archive and mark as read". Empty for hide and bundle. */
export function verbs(rule: Rule): string {
  return cap(list(rule.verbs.map((v) => VERBS[v] ?? v)));
}

/** Each condition of a rule, and its job, as a short label. */
export function conditions(rule: Rule): string[] {
  const out: string[] = [];
  for (const [key, value] of Object.entries(rule.when)) {
    const v = String(value);
    if (key === "form") out.push(FORMS[v] ?? v);
    else if (key === "sender") out.push(`From ${v}`);
    else if (key === "sender_kind") out.push(KINDS[v] ?? v);
    else if (key === "label") out.push(`Label ${v}`);
    else if (key === "record_type" || key === "commitment_kind") out.push(PLURAL[v] ?? `${cap(v)}s`);
    else if (key === "author") out.push(v === "person" ? "By you" : `By ${v}`);
    else if (key === "older_than_days") out.push(`Older than ${v} ${v === "1" ? "day" : "days"}`);
    else out.push(`${key} ${v}`);
  }
  if (rule.job) out.push(`Job ${rule.job}`);
  return out;
}

/** The groups of the view, in the order of the evaluation (spec 2 section 6.4): the strict effect first. */
export const GROUPS: { effect: Rule["effect"]; title: string; note: string }[] = [
  { effect: "deny", title: "Never", note: "Iris does not make these changes." },
  { effect: "review", title: "Ask first", note: "These changes wait for you, also when another rule allows them." },
  { effect: "allow", title: "Allowed", note: "In apply mode, Iris makes these changes by itself. Its fixed safety limits still apply." },
  { effect: "hide", title: "Hidden", note: "The brief does not show these items. Iris keeps them." },
  { effect: "bundle", title: "Bundled", note: "The brief shows these items together, as one line." },
];

export const MODES = {
  observe: { title: "Observe", note: "Iris changes nothing. It shows what it would do." },
  apply: {
    title: "Apply",
    note: "Iris makes the changes that your rules allow. It never makes a change that a Never rule or a safety limit forbids. All other changes wait for you.",
  },
} as const;

/**
 * An example rule that shows the feed as one line in the brief, with the ID
 * `id` that no rule had. With `mode` (no file yet), a complete first file:
 * the parser needs `version` and `mode`.
 */
export function example(date: string, mode: Mode | null, id: number): string {
  const rule = `[[rule]]
id = ${id}
name = "Bundle the feed"
effect = "bundle"
when = { form = "feed" }
by = "person"
added = ${date}`;
  return mode ? `version = 0\nmode = "${mode}"\n\n${rule}` : rule;
}
