<script lang="ts">
  let { html, text, senderAddress = '' }: { html?: string | null; text?: string | null; senderAddress?: string } = $props();

  let iframeEl: HTMLIFrameElement;
  let showRemoteImages = $state(false);
  let blockedImageCount = $state(0);

  const TRUSTED_SENDERS_KEY = 'iris-trusted-image-senders';

  function getSenderDomain(address: string): string {
    const match = address.match(/@([\w.-]+)/);
    return match ? match[1].toLowerCase() : '';
  }

  function getTrustedDomains(): string[] {
    try {
      return JSON.parse(localStorage.getItem(TRUSTED_SENDERS_KEY) || '[]');
    } catch {
      return [];
    }
  }

  function trustDomain(domain: string) {
    if (!domain) return;
    const current = getTrustedDomains();
    if (!current.includes(domain)) {
      current.push(domain);
      localStorage.setItem(TRUSTED_SENDERS_KEY, JSON.stringify(current));
    }
  }

  // On mount: check if sender's domain is already trusted
  $effect(() => {
    if (senderAddress && !showRemoteImages) {
      const domain = getSenderDomain(senderAddress);
      if (domain && getTrustedDomains().includes(domain)) {
        showRemoteImages = true;
      }
    }
  });

  function handleLoadImages() {
    showRemoteImages = true;
  }

  function handleAlwaysLoad() {
    const domain = getSenderDomain(senderAddress);
    trustDomain(domain);
    showRemoteImages = true;
  }

  function sanitizeHtml(raw: string): string {
    // Security model: the iframe sandbox (no allow-scripts, no allow-forms,
    // no allow-top-navigation) is the primary boundary. We strip <script>,
    // event handlers, and dangerous tags as defense-in-depth, but preserve
    // all CSS (<style>, @font-face, url(), class selectors) since email
    // layout depends on it. A CSP injected into the iframe blocks outbound
    // network fetches by default, so preserving CSS no longer leaks opens.
    return raw
      .replace(/<script[\s\S]*?<\/script>/gi, '')
      .replace(/<(iframe|object|embed|applet|form|input|textarea|select|button)[\s\S]*?<\/\1>/gi, '')
      .replace(/<(iframe|object|embed|applet|form|input|textarea|select|button)\b[^>]*\/?>/gi, '')
      .replace(/\s+on\w+\s*=\s*("[^"]*"|'[^']*'|[^\s>]*)/gi, '')
      .replace(/<meta\s+http-equiv\s*=\s*["']?refresh["']?[^>]*>/gi, '');
  }

  /**
   * Strip remote images from HTML, replacing them with a placeholder.
   * Preserves inline/embedded images (data: URIs and cid: references).
   * Also blocks CSS background-image with remote URLs in style attributes.
   * Returns the processed HTML and the count of blocked images.
   */
  function stripRemoteImages(raw: string): { html: string; count: number } {
    let count = 0;

    // Replace remote <img> tags — keep data: and cid: src values
    const processed = raw.replace(/<img\b([^>]*)>/gi, (match, attrs: string) => {
      const srcMatch = attrs.match(/src\s*=\s*(?:"([^"]*)"|'([^']*)'|(\S+))/i);
      if (!srcMatch) return match; // no src — leave as-is
      const src = srcMatch[1] ?? srcMatch[2] ?? srcMatch[3] ?? '';

      // Preserve inline/embedded images
      if (src.startsWith('data:') || src.startsWith('cid:')) {
        return match;
      }

      // Block remote images (http/https or protocol-relative)
      if (src.startsWith('http:') || src.startsWith('https:') || src.startsWith('//')) {
        count++;
        // Extract alt text if available
        const altMatch = attrs.match(/alt\s*=\s*(?:"([^"]*)"|'([^']*)')/i);
        const alt = altMatch ? (altMatch[1] ?? altMatch[2] ?? '') : '';
        const label = alt || 'Image blocked';
        return `<span style="display:inline-flex;align-items:center;gap:4px;padding:2px 8px;border:1px dashed #ccc;border-radius:4px;font-size:12px;color:#888;background:#f5f5f5;margin:2px 0;" title="${src.replace(/"/g, '&quot;')}">` +
          `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#999" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/><line x1="2" y1="2" x2="22" y2="22"/></svg>` +
          `${label.replace(/</g, '&lt;').replace(/>/g, '&gt;')}</span>`;
      }

      return match; // relative or other — leave as-is
    });

    // Block CSS background-image with remote URLs in style attributes
    const withBgBlocked = processed.replace(
      /style\s*=\s*"([^"]*)"/gi,
      (match, styleContent: string) => {
        // Replace background-image: url(http...) and background: ... url(http...) patterns
        const cleaned = styleContent.replace(
          /background(?:-image)?\s*:[^;]*url\(\s*(?:["']?)(https?:\/\/[^"')]+)(?:["']?)\s*\)[^;]*/gi,
          (bgMatch) => {
            count++;
            return '/* remote bg blocked */';
          }
        );
        if (cleaned !== styleContent) {
          return `style="${cleaned}"`;
        }
        return match;
      }
    );

    return { html: withBgBlocked, count };
  }

  function getTextContent(raw: string): string {
    const escaped = raw
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;');
    return `<pre style="white-space:pre-wrap;word-wrap:break-word;font-family:inherit;">${escaped}</pre>`;
  }

  function escapeAttribute(value: string): string {
    return value
      .replace(/&/g, '&amp;')
      .replace(/"/g, '&quot;');
  }

  function buildEmailCsp(allowRemoteImages: boolean): string {
    const imgSrc = allowRemoteImages ? "img-src data: cid: https: http:;" : "img-src data: cid:;";
    return [
      "default-src 'none'",
      "base-uri 'none'",
      "form-action 'none'",
      "frame-src 'none'",
      "connect-src 'none'",
      "media-src 'none'",
      "object-src 'none'",
      "script-src 'none'",
      "style-src 'unsafe-inline'",
      "font-src https: data:",
      imgSrc,
    ].join('; ');
  }

  function injectHeadPrelude(raw: string, prelude: string): string {
    if (/<head[\s>]/i.test(raw)) {
      return raw.replace(/<head([^>]*)>/i, `<head$1>${prelude}`);
    }
    if (/<html[\s>]/i.test(raw)) {
      return raw.replace(/<html([^>]*)>/i, `<html$1><head>${prelude}</head>`);
    }
    return `<!DOCTYPE html><html><head>${prelude}</head><body>${raw}</body></html>`;
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
        let newBlockedCount = 0;
        const cspMeta = `<meta http-equiv="Content-Security-Policy" content="${escapeAttribute(buildEmailCsp(showRemoteImages))}">`;
        const headPrelude = `${cspMeta}${baseStyle}`;

        if (html) {
          const sanitized = sanitizeHtml(html);

          let finalHtml: string;
          if (showRemoteImages) {
            finalHtml = sanitized;
          } else {
            const result = stripRemoteImages(sanitized);
            finalHtml = result.html;
            newBlockedCount = result.count;
          }

          content = injectHeadPrelude(finalHtml, headPrelude);
        } else if (text) {
          content = `<!DOCTYPE html><html><head>${headPrelude}</head><body>${getTextContent(text)}</body></html>`;
        } else {
          content = `<!DOCTYPE html><html><head>${headPrelude}</head><body><p style="color:#999;">No content</p></body></html>`;
        }

        blockedImageCount = newBlockedCount;

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

{#if blockedImageCount > 0 && !showRemoteImages}
  <div class="image-block-bar">
    <div class="image-block-info">
      <svg class="image-block-icon" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="3" y="3" width="18" height="18" rx="2" ry="2"/>
        <circle cx="8.5" cy="8.5" r="1.5"/>
        <polyline points="21 15 16 10 5 21"/>
        <line x1="2" y1="2" x2="22" y2="22"/>
      </svg>
      <span>{blockedImageCount} remote image{blockedImageCount > 1 ? 's' : ''} blocked</span>
    </div>
    <div class="image-block-actions">
      <button class="load-images-btn" onclick={handleLoadImages}>
        Load images
      </button>
      {#if senderAddress}
        <button class="load-images-btn always-btn" onclick={handleAlwaysLoad}>
          Always load from this sender
        </button>
      {/if}
    </div>
  </div>
{/if}

<iframe
  bind:this={iframeEl}
  sandbox="allow-same-origin"
  class="w-full border-0 min-h-[100px]"
  title="Email content"
></iframe>

<style>
  .image-block-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    margin-bottom: 8px;
    border-radius: var(--iris-radius-md);
    background: color-mix(in srgb, var(--iris-color-warning) 10%, var(--iris-color-bg-surface));
    border: 1px solid color-mix(in srgb, var(--iris-color-warning) 25%, transparent);
    gap: 8px;
  }

  .image-block-info {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--iris-color-warning);
    font-weight: 500;
    flex-shrink: 0;
  }

  .image-block-icon {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
  }

  .image-block-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  .load-images-btn {
    font-size: 12px;
    font-weight: 500;
    padding: 4px 12px;
    border-radius: var(--iris-radius-sm);
    border: 1px solid var(--iris-color-border);
    background: var(--iris-color-bg-elevated);
    color: var(--iris-color-text);
    cursor: pointer;
    transition: all var(--iris-transition-fast);
    white-space: nowrap;
  }

  .load-images-btn:hover {
    border-color: var(--iris-color-primary);
    background: color-mix(in srgb, var(--iris-color-primary) 10%, var(--iris-color-bg-elevated));
    color: var(--iris-color-primary);
  }

  .load-images-btn:active {
    background: color-mix(in srgb, var(--iris-color-primary) 20%, var(--iris-color-bg-elevated));
  }

  .always-btn {
    border-color: var(--iris-color-primary);
    color: var(--iris-color-primary);
  }

  .always-btn:hover {
    background: color-mix(in srgb, var(--iris-color-primary) 15%, var(--iris-color-bg-elevated));
  }
</style>
