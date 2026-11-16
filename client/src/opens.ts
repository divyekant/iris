// The open count of R1-22: how many times a person opened a source mail.
// Few opens mean that the items are good enough on their own (the main
// measure of Iris). The count stays in this browser: nothing leaves the machine.
// shortcut: browser storage is per browser and per device; move the count to the engine with the measures page (R5-12).

const KEY = "iris.opens";

export type Opens = { count: number; since: string | null };

// A browser can refuse storage: then the getter of localStorage throws.
type Store = Pick<Storage, "getItem" | "setItem">;
const local = (): Store | undefined => globalThis.localStorage;

export function readOpens(storage: () => Store | undefined = local): Opens {
  try {
    const value = JSON.parse(storage()?.getItem(KEY) ?? "null");
    if (value && Number.isInteger(value.count)) return value;
  } catch {
    // No storage, or a bad value: start again from zero.
  }
  return { count: 0, since: null };
}

export function countOpen(storage: () => Store | undefined = local, now = new Date()): Opens {
  const opens = readOpens(storage);
  const next = { count: opens.count + 1, since: opens.since ?? now.toISOString() };
  try {
    storage()?.setItem(KEY, JSON.stringify(next));
  } catch {
    // Private windows can refuse storage. The page still works.
  }
  return next;
}
