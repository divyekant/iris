import { writable } from 'svelte/store';

export interface ThreadContext {
  id: string;
  subject: string;
}

/** The thread currently open in ThreadView, or null if none. */
export const currentThreadContext = writable<ThreadContext | null>(null);
