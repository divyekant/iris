// Routes live in the hash (#/brief), because the engine serves one page for the client.

export type Route =
  | { view: "home" | "setup" | "brief" | "records" | "measures" }
  | { view: "list"; form: "commitment" | "conversation" | "feed" | "signal" }
  | { view: "item" | "mail"; id: string };

const LISTS = { commitments: "commitment", conversations: "conversation", feed: "feed", signals: "signal" } as const;

export function parse(hash: string): Route {
  const [name = "", id = ""] = hash.replace(/^#\/?/, "").split("/").map(decodeURIComponent);
  if (name in LISTS) return { view: "list", form: LISTS[name as keyof typeof LISTS] };
  if ((name === "item" || name === "mail") && id) return { view: name, id };
  if (name === "setup" || name === "brief" || name === "records" || name === "measures") return { view: name };
  return { view: "home" };
}

export const href = {
  item: (id: string) => `#/item/${encodeURIComponent(id)}`,
  mail: (id: string) => `#/mail/${encodeURIComponent(id)}`,
};

export const NAV = [
  ["#/brief", "Brief"],
  ["#/commitments", "Commitments"],
  ["#/conversations", "Conversations"],
  ["#/records", "Records"],
  ["#/feed", "Feed"],
  ["#/signals", "Signals"],
  ["#/measures", "Measures"],
] as const;
