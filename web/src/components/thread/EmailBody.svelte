<script lang="ts">
  let { html, text }: { html?: string | null; text?: string | null } = $props();

  let iframeEl: HTMLIFrameElement;

  function sanitizeHtml(raw: string): string {
    // Security model: the iframe sandbox (no allow-scripts, no allow-forms,
    // no allow-top-navigation) is the primary boundary. We strip <script>,
    // event handlers, and dangerous tags as defense-in-depth, but preserve
    // all CSS (<style>, @font-face, url(), class selectors) since email
    // layout depends on it. DOMPurify's CSS sanitizer is too aggressive —
    // it strips url() in @font-face, breaking marketing email layouts.
    return raw
      .replace(/<script[\s\S]*?<\/script>/gi, '')
      .replace(/<(iframe|object|embed|applet|form|input|textarea|select|button)[\s\S]*?<\/\1>/gi, '')
      .replace(/<(iframe|object|embed|applet|form|input|textarea|select|button)\b[^>]*\/?>/gi, '')
      .replace(/\s+on\w+\s*=\s*("[^"]*"|'[^']*'|[^\s>]*)/gi, '')
      .replace(/<meta\s+http-equiv\s*=\s*["']?refresh["']?[^>]*>/gi, '');
  }

  function getTextContent(raw: string): string {
    const escaped = raw
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;');
    return `<pre style="white-space:pre-wrap;word-wrap:break-word;font-family:inherit;">${escaped}</pre>`;
  }

  $effect(() => {
    if (iframeEl) {
      const doc = iframeEl.contentDocument;
      if (doc) {
        // Always render email content with white bg / dark text regardless of
        // app theme. Email authors design for light backgrounds — dark-mode
        // inversion causes invisible text (dark inline color on dark bg).
        // Gmail, Outlook, and Apple Mail all use the same approach.
        const baseStyle = `<style>body{margin:0;padding:8px;color:#333;background:#fff;}img{max-width:100%;height:auto;}</style>`;

        let content: string;
        if (html) {
          // WHOLE_DOCUMENT returns full <!DOCTYPE><html>…</html>
          // Inject our base styles at the start of <head>
          const sanitized = sanitizeHtml(html);
          content = sanitized.replace(/<head>/i, `<head>${baseStyle}`);
        } else if (text) {
          content = `<!DOCTYPE html><html><head>${baseStyle}</head><body>${getTextContent(text)}</body></html>`;
        } else {
          content = `<!DOCTYPE html><html><head>${baseStyle}</head><body><p style="color:#999;">No content</p></body></html>`;
        }

        doc.open();
        doc.write(content);
        doc.close();
        setTimeout(() => {
          if (iframeEl && doc.body) {
            iframeEl.style.height = doc.body.scrollHeight + 'px';
          }
        }, 50);
      }
    }
  });
</script>

<iframe
  bind:this={iframeEl}
  sandbox="allow-same-origin"
  class="w-full border-0 min-h-[100px]"
  title="Email content"
></iframe>
