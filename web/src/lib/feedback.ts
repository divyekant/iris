import { writable } from 'svelte/store';

export interface FeedbackItem {
  id: string;
  message: string;
  type: 'success' | 'error' | 'info';
  undoFn?: () => void;
  autoDismissMs: number;
  createdAt: number;
}

function createFeedbackStore() {
  const { subscribe, update } = writable<FeedbackItem[]>([]);
  const timers = new Map<string, ReturnType<typeof setTimeout>>();

  function add(item: Omit<FeedbackItem, 'id' | 'createdAt'>) {
    const id = `fb-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
    const entry: FeedbackItem = { ...item, id, createdAt: Date.now() };
    update(items => [...items, entry]);

    if (item.autoDismissMs > 0) {
      const timer = setTimeout(() => dismiss(id), item.autoDismissMs);
      timers.set(id, timer);
    }
    return id;
  }

  function dismiss(id: string) {
    const timer = timers.get(id);
    if (timer) { clearTimeout(timer); timers.delete(id); }
    update(items => items.filter(i => i.id !== id));
  }

  function undo(id: string) {
    const timer = timers.get(id);
    if (timer) { clearTimeout(timer); timers.delete(id); }
    let fn: (() => void) | undefined;
    update(items => {
      const item = items.find(i => i.id === id);
      fn = item?.undoFn;
      return items.filter(i => i.id !== id);
    });
    fn?.(); // Call after store update to avoid mutation during iteration
  }

  return {
    subscribe,
    success: (message: string, opts?: { undo?: () => void }) =>
      add({ message, type: 'success', undoFn: opts?.undo, autoDismissMs: opts?.undo ? 6000 : 4000 }),
    error: (message: string) =>
      add({ message, type: 'error', autoDismissMs: 6000 }),
    info: (message: string) =>
      add({ message, type: 'info', autoDismissMs: 4000 }),
    dismiss,
    undo,
  };
}

export const feedback = createFeedbackStore();
