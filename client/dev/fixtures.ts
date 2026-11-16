// Answers of the local API, made from the examples of docs/spec/schema/examples.
// The dev server (`pnpm dev --mode fixtures`) and the tests use them. They are not in the build.
import type { Brief, Item, Message, Status } from "../src/api";
import youOwe from "../../docs/spec/schema/examples/commitment-you-owe.json" with { type: "json" };
import owedToYou from "../../docs/spec/schema/examples/commitment-owed-to-you.json" with { type: "json" };
import payment from "../../docs/spec/schema/examples/commitment-payment.json" with { type: "json" };
import conversation from "../../docs/spec/schema/examples/conversation.json" with { type: "json" };
import receipt from "../../docs/spec/schema/examples/record-receipt.json" with { type: "json" };
import booking from "../../docs/spec/schema/examples/record-booking.json" with { type: "json" };
import feed from "../../docs/spec/schema/examples/feed.json" with { type: "json" };
import signal from "../../docs/spec/schema/examples/signal-code.json" with { type: "json" };

export const items = [youOwe, owedToYou, payment, conversation, receipt, booking, feed, signal] as unknown as Item[];

const of = (list: Item[]) => ({ count: list.length, items: list });
const byForm = (form: string) => items.filter((i) => i.form === form);

export const status: Status = {
  accounts: [
    {
      id: 1,
      address: "you@example.com",
      provider: "gmail",
      state: null,
      mirror_ready_at: "2026-11-16T08:00:00Z",
      sync: null,
      last_sync: { kind: "incremental", started: "2026-11-16T09:00:00Z", ended: "2026-11-16T09:00:02Z", result: "ok" },
    },
  ],
  messages: { total: 84212, fetched: 84212, understood: 84212, gone: 0 },
};

export const brief: Brief = {
  you_owe: of(byForm("commitment").filter((i) => i.direction === "you_owe")),
  owed_to_you: of(byForm("commitment").filter((i) => i.direction === "owed_to_you")),
  conversations: of(byForm("conversation")),
  records: { receipt: 1, booking: 1 },
  feed: of(byForm("feed")),
  signals: of(byForm("signal")),
  strangers: of([]),
  hostile: 2,
  not_understood: 1,
  oldest_fetched: null,
};

export function message(id: string): Message | undefined {
  const item = items.find((i) => i.sources.some((s) => s.message === id));
  if (!item) return undefined;
  const source = item.sources.find((s) => s.message === id)!;
  return {
    id,
    thread: source.thread,
    date: item.created,
    from: { name: "Example Sender", address: "sender@example.com" },
    subject: item.title,
    labels: ["INBOX"],
    to: [{ address: "you@example.com" }],
    cc: [],
    text: `${source.quote ?? item.title}\n\nThis is a fixture message, made from the open model examples.`,
    gone: false,
    hostile: [],
    items: items.filter((i) => i.sources.some((s) => s.message === id)).map((i) => i.id),
  };
}

/** The answer for one API path (no `/api/` prefix), or undefined for a 404. */
export function answer(path: string): unknown {
  const url = new URL(path, "http://local/");
  const q = url.searchParams;
  const [name, id] = url.pathname.slice(1).split("/");
  if (name === "status") return status;
  if (name === "brief") return brief;
  if (name === "items" && id) return items.find((i) => i.id === id);
  if (name === "items") {
    return { items: items.filter((i) => (!q.get("form") || i.form === q.get("form")) && (!q.get("type") || i.type === q.get("type"))), next: null };
  }
  if (name === "messages" && id) return message(id);
  if (name === "search") return { messages: [] };
  return undefined;
}
