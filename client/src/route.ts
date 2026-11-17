// Routes live in the hash (#/brief), because the engine serves one page for the client.

export type Form = "commitment" | "conversation" | "feed" | "signal";
export type SectionKey = "you_owe" | "owed_to_you" | "conversations" | "strangers" | "signals" | "feed";

export type Route =
  | { view: "home" | "setup" | "brief" | "ask" | "measures" | "rules" | "review" | "handled" | "jobs" | "matters" | "outbox" }
  | { view: "records"; type: string }
  | { view: "list"; form: Form }
  | { view: "section"; key: SectionKey }
  | { view: "bundle"; rule: number }
  | { view: "gate"; declined: boolean }
  | { view: "item" | "mail" | "job" | "matter" | "start"; id: string };

const LISTS = { commitments: "commitment", conversations: "conversation", feed: "feed", signals: "signal" } as const;

export const SECTIONS: Record<SectionKey, string> = {
  you_owe: "You owe",
  owed_to_you: "Owed to you",
  conversations: "Conversations that wait for you",
  strangers: "From senders you do not know",
  signals: "Signals",
  feed: "To read",
};

/** The canonical job ID (docs/spec/schema/job.schema.json). Up to 18 digits keeps it inside an i64. */
export const JOB_ID = /^j-[1-9]\d{0,17}$/;

/** The canonical matter ID (docs/spec/schema/matter.schema.json). */
export const MATTER_ID = /^g-[1-9]\d{0,17}$/;

export function parse(hash: string): Route {
  let name = "";
  let id = "";
  try {
    [name = "", id = ""] = hash.replace(/^#\/?/, "").split("/").map(decodeURIComponent);
  } catch {
    return { view: "home" }; // A bad escape such as "%" in the hash.
  }
  if (name in LISTS) return { view: "list", form: LISTS[name as keyof typeof LISTS] };
  // #/item/c-4/job: the start of a job from the item (card R4-10).
  const parts = hash.replace(/^#\/?/, "").split("/");
  if (name === "item" && id && parts.length === 3 && parts[2] === "job") return { view: "start", id };
  if ((name === "item" || name === "mail") && id && parts.length === 2) return { view: name, id };
  // #/brief/gate: all senders at the stranger gate. #/brief/declined: the senders that you ignored.
  if (name === "brief" && (id === "gate" || id === "declined")) return { view: "gate", declined: id === "declined" };
  if (name === "brief" && id in SECTIONS) return { view: "section", key: id as SectionKey };
  // #/brief/bundle/3: the items of bundle rule 3 (spec 2 section 6.4).
  const rule = hash.replace(/^#\/?/, "").split("/")[2] ?? "";
  if (name === "brief" && id === "bundle" && /^[1-9]\d{0,9}$/.test(rule)) return { view: "bundle", rule: Number(rule) };
  if (name === "records") return { view: "records", type: id };
  // #/jobs, or #/jobs/j-12; #/matters, or #/matters/g-4. The canonical ID and no other part.
  for (const [list, one, ID] of [["jobs", "job", JOB_ID], ["matters", "matter", MATTER_ID]] as const) {
    if (name !== list) continue;
    const parts = hash.replace(/^#\/?/, "").split("/");
    if (parts.length === 1 || (parts.length === 2 && parts[1] === "")) return { view: list };
    return parts.length === 2 && ID.test(id) ? { view: one, id } : { view: "home" };
  }
  if (name === "setup" || name === "brief" || name === "ask" || name === "measures" || name === "rules" || name === "review" || name === "handled" || name === "outbox") return { view: name };
  return { view: "home" };
}

export const href = {
  item: (id: string) => `#/item/${encodeURIComponent(id)}`,
  start: (id: string) => `#/item/${encodeURIComponent(id)}/job`,
  mail: (id: string) => `#/mail/${encodeURIComponent(id)}`,
  bundle: (rule: number) => `#/brief/bundle/${rule}`,
  job: (id: string) => `#/jobs/${encodeURIComponent(id)}`,
  matter: (id: string) => `#/matters/${encodeURIComponent(id)}`,
};

export const NAV = [
  ["#/brief", "Brief"],
  ["#/matters", "Matters"],
  ["#/ask", "Ask"],
  ["#/review", "Review"],
  ["#/outbox", "Outbox"],
  ["#/jobs", "Jobs"],
  ["#/commitments", "Commitments"],
  ["#/conversations", "Conversations"],
  ["#/records", "Records"],
  ["#/feed", "Feed"],
  ["#/signals", "Signals"],
] as const;
