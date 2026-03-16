// web/src/lib/badge-priority.ts

export interface BadgeInfo {
  label: string;
  type: 'warning' | 'error' | 'info' | 'success' | 'neutral';
}

// Priority order: needs_reply > deadline > intent > sentiment > category
export function getPrimaryBadges(message: {
  needs_reply?: boolean;
  deadline?: string;
  intent?: string;
  sentiment?: string;
  category?: string;
  labels?: string;
}): { primary: BadgeInfo | null; overflow: number } {
  const badges: BadgeInfo[] = [];

  if (message.needs_reply) {
    badges.push({ label: 'Needs Reply', type: 'warning' });
  }
  if (message.deadline) {
    badges.push({ label: `Due ${message.deadline}`, type: 'error' });
  }
  if (message.intent && message.intent !== 'informational') {
    badges.push({ label: message.intent, type: 'info' });
  }
  if (message.sentiment && message.sentiment !== 'neutral') {
    badges.push({ label: message.sentiment, type: message.sentiment === 'negative' ? 'error' : 'success' });
  }
  if (message.category && message.category !== 'primary') {
    badges.push({ label: message.category, type: 'neutral' });
  }

  return {
    primary: badges[0] ?? null,
    overflow: Math.max(0, badges.length - 1),
  };
}
