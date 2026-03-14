import { Marked } from 'marked';

const marked = new Marked({
  breaks: true,   // Convert \n to <br>
  gfm: true,      // GitHub-flavored markdown (tables, strikethrough, etc.)
});

/**
 * Render markdown to sanitized HTML for display in chat bubbles.
 * Strips dangerous tags (script, iframe, event handlers) as defense-in-depth.
 */
export function renderMarkdown(input: string): string {
  const raw = marked.parse(input) as string;
  return sanitize(raw);
}

function sanitize(html: string): string {
  return html
    .replace(/<script[\s\S]*?<\/script>/gi, '')
    .replace(/<(iframe|object|embed|applet|form|input|textarea|select|button)[\s\S]*?<\/\1>/gi, '')
    .replace(/<(iframe|object|embed|applet|form|input|textarea|select|button)\b[^>]*\/?>/gi, '')
    .replace(/\s+on\w+\s*=\s*("[^"]*"|'[^']*'|[^\s>]*)/gi, '')
    .replace(/<meta\s+http-equiv\s*=\s*["']?refresh["']?[^>]*>/gi, '');
}
