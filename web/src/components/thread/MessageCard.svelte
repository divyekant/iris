<script lang="ts">
  import EmailBody from './EmailBody.svelte';
  import TrustBadge from '../TrustBadge.svelte';
  import ContactPopover from '../contacts/ContactPopover.svelte';

  let { message }: { message: any } = $props();
  let expanded = $state(true);
  let showContactPopover = $state(false);

  function handleSenderClick(e: MouseEvent) {
    e.stopPropagation();
    if (message.from_address) {
      showContactPopover = true;
    }
  }

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

<div class="rounded-lg message-card">
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="w-full flex items-center justify-between p-4 text-left transition-colors message-card-header cursor-pointer"
    role="button"
    tabindex="0"
    onclick={() => (expanded = !expanded)}
    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); expanded = !expanded; } }}
  >
    <div class="flex items-center gap-3 min-w-0">
      <button class="font-medium text-sm truncate sender-name-btn" onclick={handleSenderClick}>
        {message.from_name || message.from_address || 'Unknown'}
      </button>
      <span class="text-xs flex-shrink-0" style="color: var(--iris-color-text-faint);">
        {message.date ? formatDate(message.date) : ''}
      </span>
    </div>
    <span class="ml-2 flex-shrink-0" style="color: var(--iris-color-text-muted);">{expanded ? '\u25BE' : '\u25B8'}</span>
  </div>

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

{#if showContactPopover && message.from_address}
  <ContactPopover
    email={message.from_address}
    name={message.from_name}
    onclose={() => showContactPopover = false}
  />
{/if}

<style>
  .sender-name-btn {
    color: var(--iris-color-text);
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: left;
    transition: color var(--iris-transition-fast);
  }
  .sender-name-btn:hover {
    color: var(--iris-color-primary);
  }
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
</style>
