<script lang="ts">
  import { api, getSessionToken } from '../../lib/api';
  import EmailBody from './EmailBody.svelte';
  import TrustBadge from '../TrustBadge.svelte';

  let { message }: { message: any } = $props();
  let expanded = $state(true);
  let loadedAttachments = $state<any[]>([]);
  let attachmentsLoaded = $state(false);
  let unsubscribing = $state(false);
  let unsubscribeResult = $state<string | null>(null);

  function formatDate(ts: number): string {
    return new Date(ts * 1000).toLocaleString([], {
      month: 'short',
      day: 'numeric',
      year: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
    });
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function parseAddresses(json: string | null): string {
    if (!json) return '';
    try {
      return JSON.parse(json).join(', ');
    } catch {
      return json;
    }
  }

  function getFileIcon(contentType: string): string {
    if (contentType.startsWith('image/')) return '\u{1F5BC}';   // framed picture
    if (contentType.includes('pdf')) return '\u{1F4C4}';         // page facing up
    if (contentType.includes('zip') || contentType.includes('compressed') || contentType.includes('archive')) return '\u{1F4E6}'; // package
    if (contentType.includes('spreadsheet') || contentType.includes('csv') || contentType.includes('excel')) return '\u{1F4CA}'; // bar chart
    if (contentType.includes('word') || contentType.includes('document') || contentType.startsWith('text/')) return '\u{1F4DD}'; // memo
    return '\u{1F4CE}'; // paperclip
  }

  function getIconColor(contentType: string): string {
    if (contentType.startsWith('image/')) return 'var(--iris-color-info)';
    if (contentType.includes('pdf') || contentType.includes('word') || contentType.includes('document') || contentType.startsWith('text/')) return 'var(--iris-color-warning)';
    return 'var(--iris-color-text-muted)';
  }

  function downloadAttachment(attachmentId: string) {
    const url = api.attachments.downloadUrl(attachmentId);
    const token = getSessionToken();
    // Use fetch + blob to pass session token
    fetch(url, {
      headers: token ? { 'x-session-token': token } : {},
    })
      .then((res) => {
        if (!res.ok) throw new Error('Download failed');
        const disposition = res.headers.get('content-disposition');
        const filenameMatch = disposition?.match(/filename="(.+?)"/);
        const filename = filenameMatch?.[1] || 'download';
        return res.blob().then((blob) => ({ blob, filename }));
      })
      .then(({ blob, filename }) => {
        const a = document.createElement('a');
        a.href = URL.createObjectURL(blob);
        a.download = filename;
        a.click();
        URL.revokeObjectURL(a.href);
      })
      .catch(() => {
        // Fallback: open in new tab
        window.open(api.attachments.downloadUrl(attachmentId), '_blank');
      });
  }

  async function handleUnsubscribe() {
    unsubscribing = true;
    unsubscribeResult = null;
    try {
      const res = await api.messages.unsubscribe(message.id);
      if (res.method === 'one-click') {
        unsubscribeResult = res.success ? 'Unsubscribed successfully.' : 'Unsubscribe request sent.';
      } else if (res.method === 'url' && res.url) {
        window.open(res.url, '_blank', 'noopener,noreferrer');
        unsubscribeResult = 'Opened unsubscribe page in new tab.';
      } else if (res.method === 'mailto' && res.url) {
        window.location.href = res.url;
        unsubscribeResult = 'Opening email client to unsubscribe.';
      }
    } catch {
      unsubscribeResult = 'Failed to unsubscribe.';
    } finally {
      unsubscribing = false;
    }
  }

  // Load attachments from the API when expanded and message has attachments
  $effect(() => {
    if (expanded && message.has_attachments && !attachmentsLoaded) {
      attachmentsLoaded = true;
      api.messages.attachments(message.id).then((atts) => {
        loadedAttachments = atts;
      }).catch(() => {
        // Fall back to metadata-only display
        loadedAttachments = [];
      });
    }
  });

  // Determine which attachments to display: API-loaded (with download) or metadata-only fallback
  let displayAttachments = $derived(
    loadedAttachments.length > 0
      ? loadedAttachments
      : (message.attachments || [])
  );
  let hasDownloadIds = $derived(loadedAttachments.length > 0);
</script>

<div class="rounded-lg message-card">
  <button
    class="w-full flex items-center justify-between p-4 text-left transition-colors message-card-header"
    onclick={() => (expanded = !expanded)}
  >
    <div class="flex items-center gap-3 min-w-0">
      <span class="font-medium text-sm truncate" style="color: var(--iris-color-text);">
        {message.from_name || message.from_address || 'Unknown'}
      </span>
      <span class="text-xs flex-shrink-0" style="color: var(--iris-color-text-faint);">
        {message.date ? formatDate(message.date) : ''}
      </span>
    </div>
    <span class="ml-2 flex-shrink-0" style="color: var(--iris-color-text-muted);">{expanded ? '\u25BE' : '\u25B8'}</span>
  </button>

  {#if expanded}
    <div class="px-4 pb-4">
      <div class="text-xs mb-3 space-y-0.5" style="color: var(--iris-color-text-faint);">
        {#if message.to_addresses}
          <div>To: {parseAddresses(message.to_addresses)}</div>
        {/if}
        {#if message.cc_addresses}
          <div>Cc: {parseAddresses(message.cc_addresses)}</div>
        {/if}
      </div>

      {#if message.trust || (message.tracking_pixels && message.tracking_pixels.length > 0) || message.impersonation_risk}
        <div class="mb-3">
          <TrustBadge trust={message.trust || {}} trackingPixels={message.tracking_pixels || []} impersonationRisk={message.impersonation_risk || null} />
        </div>
      {/if}

      {#if message.list_unsubscribe}
        <div class="mb-3 flex items-center gap-2">
          {#if unsubscribeResult}
            <span class="text-xs" style="color: var(--iris-color-text-faint);">{unsubscribeResult}</span>
          {:else}
            <button
              class="text-xs px-3 py-1 rounded-md transition-colors unsubscribe-btn"
              onclick={handleUnsubscribe}
              disabled={unsubscribing}
            >
              {unsubscribing ? 'Unsubscribing...' : 'Unsubscribe'}
            </button>
            <span class="text-xs" style="color: var(--iris-color-text-faint);">from this mailing list</span>
          {/if}
        </div>
      {/if}

      <EmailBody html={message.body_html} text={message.body_text} />

      {#if displayAttachments.length > 0}
        <div class="mt-4 pt-3 attachment-bar">
          <p class="text-xs font-medium mb-2" style="color: var(--iris-color-text-muted);">
            Attachments ({displayAttachments.length})
          </p>
          <div class="flex flex-wrap gap-2">
            {#each displayAttachments as att}
              {@const ct = att.content_type || att.mime_type || 'application/octet-stream'}
              {#if hasDownloadIds && att.id}
                <button
                  class="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs attachment-chip attachment-download"
                  onclick={() => downloadAttachment(att.id)}
                  title="Download {att.filename || 'file'}"
                >
                  <span style="color: {getIconColor(ct)};">{getFileIcon(ct)}</span>
                  <span class="truncate max-w-[200px]">{att.filename || 'unnamed'}</span>
                  <span style="color: var(--iris-color-text-faint);">({formatSize(att.size)})</span>
                  <span class="download-icon" style="color: var(--iris-color-primary);">{'\u2913'}</span>
                </button>
              {:else}
                <div
                  class="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs attachment-chip"
                >
                  <span style="color: {getIconColor(ct)};">{getFileIcon(ct)}</span>
                  <span class="truncate max-w-[200px]">{att.filename || 'unnamed'}</span>
                  <span style="color: var(--iris-color-text-faint);">({formatSize(att.size)})</span>
                </div>
              {/if}
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .message-card {
    border: 1px solid var(--iris-color-border-subtle);
    background: var(--iris-color-bg-elevated);
  }
  .message-card-header:hover {
    background: var(--iris-color-bg-surface);
  }
  .attachment-bar {
    border-top: 1px solid var(--iris-color-border);
    background: var(--iris-color-bg-surface);
    border-radius: 0 0 8px 8px;
    margin: 0 -16px -16px;
    padding: 12px 16px 16px;
  }
  .attachment-chip {
    background: var(--iris-color-bg-elevated);
    color: var(--iris-color-text-muted);
    border: 1px solid var(--iris-color-border-subtle);
  }
  .attachment-download {
    cursor: pointer;
    transition: all 120ms ease;
  }
  .attachment-download:hover {
    border-color: var(--iris-color-primary);
    background: color-mix(in srgb, var(--iris-color-primary) 8%, var(--iris-color-bg-elevated));
  }
  .download-icon {
    margin-left: 4px;
    font-size: 14px;
  }
  .unsubscribe-btn {
    background: var(--iris-color-bg-surface);
    color: var(--iris-color-text-muted);
    border: 1px solid var(--iris-color-border);
  }
  .unsubscribe-btn:hover:not(:disabled) {
    background: var(--iris-color-bg-elevated);
    color: var(--iris-color-text);
    border-color: var(--iris-color-border-subtle);
  }
  .unsubscribe-btn:disabled {
    opacity: 0.5;
  }
</style>
