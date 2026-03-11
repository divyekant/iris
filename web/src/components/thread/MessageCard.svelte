<script lang="ts">
  import { api } from '../../lib/api';
  import EmailBody from './EmailBody.svelte';
  import TrustBadge from '../TrustBadge.svelte';

  let { message }: { message: any } = $props();
  let expanded = $state(true);
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

      {#if message.trust || (message.tracking_pixels && message.tracking_pixels.length > 0)}
        <div class="mb-3">
          <TrustBadge trust={message.trust || {}} trackingPixels={message.tracking_pixels || []} />
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

      {#if message.attachments && message.attachments.length > 0}
        <div class="mt-4 pt-3" style="border-top: 1px solid var(--iris-color-border-subtle);">
          <p class="text-xs font-medium mb-2" style="color: var(--iris-color-text-muted);">
            Attachments ({message.attachments.length})
          </p>
          <div class="flex flex-wrap gap-2">
            {#each message.attachments as att}
              <div
                class="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs attachment-chip"
              >
                <span>{'\u{1F4CE}'}</span>
                <span class="truncate max-w-[200px]">{att.filename}</span>
                <span style="color: var(--iris-color-text-faint);">({formatSize(att.size)})</span>
              </div>
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
  .attachment-chip {
    background: var(--iris-color-bg-surface);
    color: var(--iris-color-text-muted);
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
