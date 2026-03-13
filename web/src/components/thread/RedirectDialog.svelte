<script lang="ts">
  import { api } from '../../lib/api';
  import { Forward } from 'lucide-svelte';

  let {
    messageId,
    fromAddress = '',
    subject = '',
    onclose,
  }: {
    messageId: string;
    fromAddress: string;
    subject: string;
    onclose: () => void;
  } = $props();

  let to = $state('');
  let sending = $state(false);
  let error = $state('');
  let success = $state(false);

  async function handleRedirect() {
    const trimmed = to.trim();
    if (!trimmed || !trimmed.includes('@')) {
      error = 'Please enter a valid email address.';
      return;
    }

    sending = true;
    error = '';
    try {
      await api.messages.redirect(messageId, trimmed);
      success = true;
      setTimeout(() => {
        onclose();
      }, 1500);
    } catch (e: any) {
      const msg = e.message || 'Failed to redirect';
      if (msg.includes('502')) {
        error = 'Redirect failed — check account connection in Settings.';
      } else if (msg.includes('400')) {
        error = 'Invalid recipient address.';
      } else if (msg.includes('404')) {
        error = 'Message not found.';
      } else {
        error = msg;
      }
    } finally {
      sending = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onclose();
    }
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      handleRedirect();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed inset-0 z-50 flex items-center justify-center"
  onkeydown={handleKeydown}
>
  <!-- Backdrop -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="absolute inset-0 redirect-backdrop"
    onclick={onclose}
  ></div>

  <!-- Dialog -->
  <div class="relative w-full max-w-md rounded-xl p-6 space-y-4 redirect-dialog">
    <!-- Header -->
    <div class="flex items-center gap-2">
      <Forward size={18} style="color: var(--iris-color-primary);" />
      <h3 class="text-base font-semibold" style="color: var(--iris-color-text);">Redirect Message</h3>
    </div>

    <!-- Original info -->
    <div class="space-y-2 rounded-lg p-3" style="background: var(--iris-color-bg-surface);">
      <div class="flex items-start gap-2">
        <span class="text-xs w-12 shrink-0" style="color: var(--iris-color-text-faint);">From</span>
        <span class="text-sm truncate" style="color: var(--iris-color-text-muted);">{fromAddress || '(unknown)'}</span>
      </div>
      <div class="flex items-start gap-2">
        <span class="text-xs w-12 shrink-0" style="color: var(--iris-color-text-faint);">Subject</span>
        <span class="text-sm truncate" style="color: var(--iris-color-text-muted);">{subject || '(no subject)'}</span>
      </div>
    </div>

    <!-- Redirect To -->
    <div class="space-y-1">
      <label for="redirect-to" class="text-xs font-medium" style="color: var(--iris-color-text-faint);">
        Redirect To
      </label>
      <input
        id="redirect-to"
        type="email"
        bind:value={to}
        class="w-full text-sm rounded-lg px-3 py-2 outline-none redirect-input"
        placeholder="recipient@example.com"
        disabled={sending || success}
      />
    </div>

    <!-- Error -->
    {#if error}
      <p class="text-xs" style="color: var(--iris-color-error);">{error}</p>
    {/if}

    <!-- Success -->
    {#if success}
      <p class="text-xs" style="color: var(--iris-color-success);">Message redirected successfully.</p>
    {/if}

    <!-- Actions -->
    <div class="flex items-center justify-end gap-2 pt-1">
      <span class="text-[10px] mr-auto" style="color: var(--iris-color-text-faint);">
        {navigator.platform.includes('Mac') ? '\u2318' : 'Ctrl'}+Enter to send
      </span>
      <button
        class="px-4 py-2 text-sm rounded-lg font-medium transition-colors redirect-cancel-btn"
        onclick={onclose}
        disabled={sending}
      >
        Cancel
      </button>
      <button
        class="px-4 py-2 text-sm rounded-lg font-medium transition-colors flex items-center gap-1.5 redirect-send-btn disabled:opacity-50"
        onclick={handleRedirect}
        disabled={sending || success || !to.trim()}
      >
        {#if sending}
          <div class="w-3 h-3 rounded-full animate-spin" style="border: 2px solid var(--iris-color-border-subtle); border-top-color: var(--iris-color-bg);"></div>
          Redirecting...
        {:else}
          <Forward size={14} />
          Redirect
        {/if}
      </button>
    </div>
  </div>
</div>

<style>
  .redirect-backdrop {
    background: var(--iris-color-overlay);
  }
  .redirect-dialog {
    background: var(--iris-color-bg-elevated);
    border: 1px solid var(--iris-color-border);
    box-shadow: 0 20px 60px var(--iris-color-overlay);
  }
  .redirect-input {
    background: var(--iris-color-bg);
    color: var(--iris-color-text);
    border: 1px solid var(--iris-color-border);
  }
  .redirect-input:focus {
    border-color: var(--iris-color-primary);
  }
  .redirect-input:disabled {
    opacity: 0.6;
  }
  .redirect-cancel-btn {
    background: var(--iris-color-bg-surface);
    color: var(--iris-color-text-muted);
    border: 1px solid var(--iris-color-border);
  }
  .redirect-cancel-btn:hover:not(:disabled) {
    background: var(--iris-color-bg-elevated);
    color: var(--iris-color-text);
  }
  .redirect-send-btn {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .redirect-send-btn:hover:not(:disabled) {
    background: var(--iris-color-primary-hover);
  }
</style>
