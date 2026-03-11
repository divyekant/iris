<script lang="ts">
  import { X } from 'lucide-svelte';
  import ResponseTimeCard from './ResponseTimeCard.svelte';

  let { email, name = null, onclose }: {
    email: string;
    name?: string | null;
    onclose: () => void;
  } = $props();

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

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="popover-backdrop"
  onclick={handleBackdrop}
  onkeydown={handleKeydown}
  role="dialog"
  tabindex="-1"
  aria-label="Contact details for {name || email}"
>
  <div class="popover-panel">
    <div class="popover-header">
      <div class="contact-info">
        <div class="contact-avatar">
          {(name || email).charAt(0).toUpperCase()}
        </div>
        <div class="contact-text">
          {#if name}
            <p class="contact-name">{name}</p>
          {/if}
          <p class="contact-email">{email}</p>
        </div>
      </div>
      <button class="close-btn" onclick={onclose} title="Close" aria-label="Close contact panel">
        <X size={14} />
      </button>
    </div>

    <div class="popover-body">
      <ResponseTimeCard {email} />
    </div>
  </div>
</div>

<style>
  .popover-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 80px;
    background: var(--iris-color-overlay);
  }

  .popover-panel {
    width: 320px;
    max-height: calc(100vh - 120px);
    overflow-y: auto;
    background: var(--iris-color-bg-surface);
    border: 1px solid var(--iris-color-border);
    border-radius: var(--iris-radius-lg);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.3);
  }

  .popover-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 16px;
    border-bottom: 1px solid var(--iris-color-border-subtle);
  }

  .contact-info {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }

  .contact-avatar {
    width: 36px;
    height: 36px;
    border-radius: var(--iris-radius-full);
    background: color-mix(in srgb, var(--iris-color-primary) 15%, transparent);
    color: var(--iris-color-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 14px;
    font-weight: 700;
    flex-shrink: 0;
  }

  .contact-text {
    min-width: 0;
  }

  .contact-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--iris-color-text);
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .contact-email {
    font-size: 12px;
    color: var(--iris-color-text-faint);
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .close-btn {
    padding: 4px;
    color: var(--iris-color-text-faint);
    transition: color var(--iris-transition-fast);
    background: none;
    border: none;
    cursor: pointer;
    flex-shrink: 0;
  }

  .close-btn:hover {
    color: var(--iris-color-text);
  }

  .popover-body {
    padding: 12px;
  }
</style>
