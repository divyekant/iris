import { writable } from 'svelte/store';

export interface Command {
  id: string;
  label: string;
  category: string;  // 'Navigation', 'Actions', 'AI', 'Settings'
  keywords: string[];
  action: () => void;
  shortcut?: string;
}

const commands: Command[] = [];

export const paletteOpen = writable(false);

export function registerCommand(cmd: Command) {
  const idx = commands.findIndex(c => c.id === cmd.id);
  if (idx >= 0) commands[idx] = cmd;
  else commands.push(cmd);
}

export function registerCommands(cmds: Command[]) {
  cmds.forEach(registerCommand);
}

export function unregisterCommand(id: string) {
  const idx = commands.findIndex(c => c.id === id);
  if (idx >= 0) commands.splice(idx, 1);
}

export function searchCommands(query: string): Command[] {
  if (!query.trim()) return commands.slice(0, 10);
  const q = query.toLowerCase();
  return commands.filter(c =>
    c.label.toLowerCase().includes(q) ||
    c.category.toLowerCase().includes(q) ||
    c.keywords.some(k => k.toLowerCase().includes(q))
  ).slice(0, 10);
}

export function togglePalette() {
  paletteOpen.update(v => !v);
}
