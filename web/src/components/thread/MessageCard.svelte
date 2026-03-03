<script lang="ts">
  import EmailBody from './EmailBody.svelte';
  import TrustBadge from '../TrustBadge.svelte';

  let { message }: { message: any } = $props();
  let expanded = $state(true);

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
</script>

<div class="border border-gray-200 dark:border-gray-700 rounded-lg bg-white dark:bg-gray-900">
  <button
    class="w-full flex items-center justify-between p-4 text-left hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors"
    onclick={() => (expanded = !expanded)}
  >
    <div class="flex items-center gap-3 min-w-0">
      <span class="font-medium text-sm truncate">
        {message.from_name || message.from_address || 'Unknown'}
      </span>
      <span class="text-xs text-gray-400 flex-shrink-0">
        {message.date ? formatDate(message.date) : ''}
      </span>
    </div>
    <span class="text-gray-400 ml-2 flex-shrink-0">{expanded ? '\u25BE' : '\u25B8'}</span>
  </button>

  {#if expanded}
    <div class="px-4 pb-4">
      <div class="text-xs text-gray-500 dark:text-gray-400 mb-3 space-y-0.5">
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

      <EmailBody html={message.body_html} text={message.body_text} />

      {#if message.attachments && message.attachments.length > 0}
        <div class="mt-4 pt-3 border-t border-gray-100 dark:border-gray-800">
          <p class="text-xs font-medium text-gray-600 dark:text-gray-400 mb-2">
            Attachments ({message.attachments.length})
          </p>
          <div class="flex flex-wrap gap-2">
            {#each message.attachments as att}
              <div
                class="flex items-center gap-1.5 px-3 py-1.5 bg-gray-50 dark:bg-gray-800 rounded text-xs text-gray-600 dark:text-gray-400"
              >
                <span>{'\u{1F4CE}'}</span>
                <span class="truncate max-w-[200px]">{att.filename}</span>
                <span class="text-gray-400">({formatSize(att.size)})</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>
