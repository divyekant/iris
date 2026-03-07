<script lang="ts">
  import DOMPurify from 'dompurify';

  let { html, text }: { html?: string | null; text?: string | null } = $props();

  let iframeEl: HTMLIFrameElement;

  function getSanitizedContent(): string {
    if (html) {
      return DOMPurify.sanitize(html, {
        ALLOWED_TAGS: [
          'p', 'br', 'b', 'i', 'u', 'strong', 'em', 'a', 'ul', 'ol', 'li',
          'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'blockquote', 'pre', 'code',
          'table', 'thead', 'tbody', 'tr', 'th', 'td', 'div', 'span', 'img',
          'hr', 'sub', 'sup', 'center', 'font', 's', 'strike',
        ],
        ALLOWED_ATTR: [
          'href', 'src', 'alt', 'target', 'width', 'height',
          'colspan', 'rowspan', 'style',
          'align', 'valign', 'bgcolor', 'color', 'size', 'face',
          'border', 'cellpadding', 'cellspacing',
        ],
        ALLOW_DATA_ATTR: false,
      });
    }
    if (text) {
      const escaped = text
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
      return `<pre style="white-space:pre-wrap;word-wrap:break-word;font-family:inherit;">${escaped}</pre>`;
    }
    return '<p style="color:#999;">No content</p>';
  }

  $effect(() => {
    if (iframeEl) {
      const doc = iframeEl.contentDocument;
      if (doc) {
        doc.open();
        doc.write(`<!DOCTYPE html>
<html><head><style>
body{margin:0;padding:8px;font-family:-apple-system,system-ui,sans-serif;font-size:14px;line-height:1.6;color:#333;background:#fff;}
a{color:#2563eb;}
img{max-width:100%;height:auto;}
blockquote{margin:8px 0;padding-left:12px;border-left:3px solid #ddd;color:#666;}
table{border-collapse:collapse;}
td,th{padding:4px 8px;}
</style></head><body>${getSanitizedContent()}</body></html>`);
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
