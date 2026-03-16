import { writable, get } from 'svelte/store';

export type KeyboardMode = 'inbox' | 'thread' | 'compose' | 'chat' | 'settings' | 'global';

export const currentMode = writable<KeyboardMode>('inbox');

interface ShortcutDef {
  key: string;
  mode: KeyboardMode | 'global';
  shift?: boolean;
  meta?: boolean;
  ctrl?: boolean;
  action: () => void;
  description: string;
}

const shortcuts: ShortcutDef[] = [];
let pendingChord = '';
let chordTimer: ReturnType<typeof setTimeout> | null = null;

export function registerShortcut(def: ShortcutDef) {
  // Deduplicate by key+mode
  const idx = shortcuts.findIndex(s => s.key === def.key && s.mode === def.mode && s.shift === def.shift && s.meta === def.meta);
  if (idx >= 0) shortcuts[idx] = def;
  else shortcuts.push(def);
}

export function registerShortcuts(defs: ShortcutDef[]) {
  defs.forEach(d => registerShortcut(d));
}

export function unregisterShortcut(key: string, mode: KeyboardMode | 'global') {
  const idx = shortcuts.findIndex(s => s.key === key && s.mode === mode);
  if (idx >= 0) shortcuts.splice(idx, 1);
}

export function getShortcutsForMode(mode: KeyboardMode): ShortcutDef[] {
  return shortcuts.filter(s => s.mode === mode || s.mode === 'global');
}

function isInputFocused(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  const tag = el.tagName.toLowerCase();
  return tag === 'input' || tag === 'textarea' || (el as HTMLElement).isContentEditable;
}

export function handleGlobalKeydown(e: KeyboardEvent) {
  // Meta/Ctrl shortcuts always fire (e.g. Cmd+K)
  if (e.metaKey || e.ctrlKey) {
    const match = shortcuts.find(s =>
      s.key === e.key && (s.meta || s.ctrl) &&
      (s.mode === get(currentMode) || s.mode === 'global')
    );
    if (match) {
      e.preventDefault();
      match.action();
      return;
    }
  }

  // Skip non-meta shortcuts when input focused (except Escape)
  if (isInputFocused() && !e.metaKey && !e.ctrlKey && e.key !== 'Escape') return;

  const mode = get(currentMode);

  // Handle chord (g+key)
  if (pendingChord === 'g') {
    pendingChord = '';
    if (chordTimer) clearTimeout(chordTimer);
    const match = shortcuts.find(s => s.key === `g+${e.key}` && (s.mode === mode || s.mode === 'global'));
    if (match) { e.preventDefault(); match.action(); return; }
  }

  if (e.key === 'g' && !e.metaKey && !e.ctrlKey && !e.shiftKey) {
    pendingChord = 'g';
    chordTimer = setTimeout(() => { pendingChord = ''; }, 1000);
    return;
  }

  // Single-key shortcuts
  const match = shortcuts.find(s =>
    s.key === e.key &&
    !s.meta && !s.ctrl &&
    (s.shift === undefined || s.shift === e.shiftKey) &&
    (s.mode === mode || s.mode === 'global')
  );
  if (match) {
    e.preventDefault();
    match.action();
  }
}

// All registered shortcuts for help overlay
export function getAllShortcuts(): { mode: string; key: string; description: string }[] {
  return shortcuts.map(s => ({
    mode: s.mode,
    key: s.meta ? `Cmd+${s.key}` : s.shift ? `Shift+${s.key}` : s.key,
    description: s.description,
  }));
}
