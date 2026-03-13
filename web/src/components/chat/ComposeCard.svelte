<script lang="ts">
  interface ComposeData {
    to: string[];
    cc?: string[];
    subject: string;
    body: string;
    reply_to_message_id?: string | null;
    tone?: string | null;
  }

  let {
    data,
    onedit,
    ondiscard,
  }: {
    data: ComposeData;
    onedit: (data: ComposeData) => void;
    ondiscard: () => void;
  } = $props();

  const bodyPreview = $derived(
    data.body.replace(/<[^>]*>/g, '').slice(0, 100) + (data.body.length > 100 ? '...' : '')
  );

  const recipientList = $derived(data.to.join(', '));
</script>

<div
  class="mt-2 rounded-lg overflow-hidden"
  style="background: var(--iris-color-bg-surface); border: 1px solid var(--iris-color-border); border-radius: var(--iris-radius-md);"
>
  <!-- Header -->
  <div
    class="px-3 py-2 flex items-center gap-2 border-b"
    style="border-color: var(--iris-color-border);"
  >
    <svg class="w-3.5 h-3.5 shrink-0" style="color: var(--iris-color-primary);" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
      <path stroke-linecap="round" stroke-linejoin="round" d="M21.75 6.75v10.5a2.25 2.25 0 0 1-2.25 2.25h-15a2.25 2.25 0 0 1-2.25-2.25V6.75m19.5 0A2.25 2.25 0 0 0 19.5 4.5h-15a2.25 2.25 0 0 0-2.25 2.25m19.5 0v.243a2.25 2.25 0 0 1-1.07 1.916l-7.5 4.615a2.25 2.25 0 0 1-2.36 0L3.32 8.91a2.25 2.25 0 0 1-1.07-1.916V6.75" />
    </svg>
    <span class="text-xs font-medium" style="color: var(--iris-color-text);">Draft Email</span>
  </div>

  <!-- Content -->
  <div class="px-3 py-2 space-y-1">
    <div class="flex gap-2 text-xs">
      <span class="shrink-0 font-medium" style="color: var(--iris-color-text-muted);">To:</span>
      <span class="truncate" style="color: var(--iris-color-text);">{recipientList}</span>
    </div>

    {#if data.cc && data.cc.length > 0}
      <div class="flex gap-2 text-xs">
        <span class="shrink-0 font-medium" style="color: var(--iris-color-text-muted);">Cc:</span>
        <span class="truncate" style="color: var(--iris-color-text);">{data.cc.join(', ')}</span>
      </div>
    {/if}

    <div class="flex gap-2 text-xs">
      <span class="shrink-0 font-medium" style="color: var(--iris-color-text-muted);">Subject:</span>
      <span class="truncate" style="color: var(--iris-color-text);">{data.subject}</span>
    </div>

    <p class="text-xs leading-relaxed pt-1" style="color: var(--iris-color-text-muted);">
      {bodyPreview}
    </p>
  </div>

  <!-- Actions -->
  <div
    class="px-3 py-2 flex items-center gap-2 border-t"
    style="border-color: var(--iris-color-border);"
  >
    <button
      class="px-3 py-1 text-xs font-medium rounded-md transition-all compose-card-edit-btn"
      style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
      onclick={() => onedit(data)}
    >
      Edit & Send
    </button>
    <button
      class="px-3 py-1 text-xs rounded-md transition-all compose-card-discard-btn"
      style="color: var(--iris-color-text-muted); border: 1px solid var(--iris-color-border);"
      onclick={ondiscard}
    >
      Discard
    </button>
  </div>
</div>

<style>
  .compose-card-edit-btn:hover {
    filter: brightness(1.1);
  }
  .compose-card-edit-btn:active {
    transform: scale(0.98);
  }
  .compose-card-discard-btn:hover {
    background: color-mix(in srgb, var(--iris-color-error) 10%, transparent);
    color: var(--iris-color-error);
    border-color: var(--iris-color-error);
  }
</style>
