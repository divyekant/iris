// Routes live in the hash (#/brief), because the engine serves one page for the client.

export type Form = "commitment" | "conversation" | "feed" | "signal";
export type SectionKey = "you_owe" | "owed_to_you" | "conversations" | "strangers" | "signals" | "feed";

export type Route =
  | { view: "home" | "setup" | "brief" | "measures" | "rules" | "review" }
  | { view: "records"; type: string }
  | { view: "list"; form: Form }
  | { view: "section"; key: SectionKey }
  | { view: "bundle"; rule: number }
  | { view: "item" | "mail"; id: string };

const LISTS = { commitments: "commitment", conversations: "conversation", feed: "feed", signals: "signal" } as const;

export const SECTIONS: Record<SectionKey, string> = {
  you_owe: "You owe",
  owed_to_you: "Owed to you",
  conversations: "Conversations that wait for you",
  strangers: "From senders you do not know",
  signals: "Signals",
  feed: "To read",
};

export function parse(hash: string): Route {
  let name = "";
  let id = "";
  try {
    [name = "", id = ""] = hash.replace(/^#\/?/, "").split("/").map(decodeURIComponent);
  } catch {
    return { view: "home" }; // A bad escape such as "%" in the hash.
  }
  if (name in LISTS) return { view: "list", form: LISTS[name as keyof typeof LISTS] };
  if ((name === "item" || name === "mail") && id) return { view: name, id };
  if (name === "brief" && id in SECTIONS) return { view: "section", key: id as SectionKey };
  // #/brief/bundle/3: the items of bundle rule 3 (spec 2 section 6.4).
  const rule = hash.replace(/^#\/?/, "").split("/")[2] ?? "";
  if (name === "brief" && id === "bundle" && /^[1-9]\d{0,9}$/.test(rule)) return { view: "bundle", rule: Number(rule) };
  if (name === "records") return { view: "records", type: id };
  if (name === "setup" || name === "brief" || name === "measures" || name === "rules" || name === "review") return { view: name };
  return { view: "home" };
}

export const href = {
  item: (id: string) => `#/item/${encodeURIComponent(id)}`,
  mail: (id: string) => `#/mail/${encodeURIComponent(id)}`,
  bundle: (rule: number) => `#/brief/bundle/${rule}`,
};

export const NAV = [
  ["#/brief", "Brief"],
  ["#/review", "Review"],
  ["#/commitments", "Commitments"],
  ["#/conversations", "Conversations"],
  ["#/records", "Records"],
  ["#/feed", "Feed"],
  ["#/signals", "Signals"],
] as const;
