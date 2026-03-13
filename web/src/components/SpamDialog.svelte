<script lang="ts">
  import { ShieldAlert } from 'lucide-svelte';

  let { senderEmail, messageIds, onconfirm, onclose }: {
    senderEmail: string;
    messageIds: string[];
    onconfirm: (blockSender: boolean) => void;
    onclose: () => void;
  } = $props();

  let blockSender = $state(true);

  function handleConfirm() {
    onconfirm(blockSender);
  }

  function handleBackdrop(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onclose();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onclose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed inset-0 z-50 flex items-center justify-center"
  style="background: var(--iris-color-overlay);"
  onclick={handleBackdrop}
>
  <div
    class="w-full max-w-sm rounded-xl p-6 space-y-4"
    style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);"
  >
    <!-- Header -->
    <div class="flex items-center gap-3">
      <div class="p-2 rounded-lg" style="background: color-mix(in srgb, var(--iris-color-warning) 15%, transparent);">
        <ShieldAlert size={20} style="color: var(--iris-color-warning);" />
      </div>
      <div>
        <h3 class="text-base font-semibold" style="color: var(--iris-color-text);">Report as Spam?</h3>
        <p class="text-xs" style="color: var(--iris-color-text-faint);">
          {messageIds.length} message{messageIds.length === 1 ? '' : 's'} will be moved to Spam
        </p>
      </div>
    </div>

    <!-- Sender info -->
    <div class="rounded-lg px-3 py-2" style="background: var(--iris-color-bg-surface);">
      <p class="text-xs" style="color: var(--iris-color-text-faint);">Sender</p>
      <p class="text-sm font-medium truncate" style="color: var(--iris-color-text);">{senderEmail}</p>
    </div>

    <!-- Block checkbox -->
    <label class="flex items-center gap-2 cursor-pointer">
      <input
        type="checkbox"
        bind:checked={blockSender}
        class="w-4 h-4 rounded"
        style="accent-color: var(--iris-color-primary);"
      />
      <span class="text-sm" style="color: var(--iris-color-text);">Also block this sender</span>
    </label>

    <!-- Actions -->
    <div class="flex justify-end gap-2 pt-2">
      <button
        class="px-4 py-2 text-sm rounded-lg transition-colors spam-cancel-btn"
        onclick={onclose}
      >
        Cancel
      </button>
      <button
        class="px-4 py-2 text-sm font-medium rounded-lg transition-colors spam-confirm-btn"
        onclick={handleConfirm}
      >
        Report Spam
      </button>
    </div>
  </div>
</div>

<style>
  .spam-cancel-btn {
    color: var(--iris-color-text-muted);
    border: 1px solid var(--iris-color-border);
    background: transparent;
  }
  .spam-cancel-btn:hover {
    background: var(--iris-color-bg-surface);
  }
  .spam-confirm-btn {
    background: var(--iris-color-error);
    color: white;
  }
  .spam-confirm-btn:hover {
    background: color-mix(in srgb, var(--iris-color-error) 85%, white);
  }
</style>
