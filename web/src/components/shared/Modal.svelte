<script lang="ts">
  import { X } from 'lucide-svelte';
  import type { Snippet } from 'svelte';
  import { irisFade, irisScale } from '$lib/transitions';

  let {
    size = 'md',
    title = '',
    onclose,
    children,
  }: {
    size?: 'sm' | 'md' | 'lg';
    title?: string;
    onclose: () => void;
    children: Snippet;
  } = $props();

  const sizeClass: Record<string, string> = {
    sm: 'modal-sm',
    md: 'modal-md',
    lg: 'modal-lg',
  };

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      onclose();
    }
  }

  function handleBackdrop(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onclose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_interactive_supports_focus -->
<div
  class="fixed inset-0 z-50 flex items-center justify-center modal-backdrop"
  onclick={handleBackdrop}
  role="dialog"
  aria-modal="true"
  aria-label={title || undefined}
  transition:irisFade
>
  <div class="w-full modal-card {sizeClass[size]}" transition:irisScale>
    {#if title}
      <div class="modal-header">
        <h3 class="text-base font-semibold modal-title">{title}</h3>
        <button
          class="modal-close-btn"
          onclick={onclose}
          title="Close"
          aria-label="Close"
        >
          <X size={16} />
        </button>
      </div>
    {/if}
    <div class="modal-body">
      {@render children()}
    </div>
  </div>
</div>

<style>
  .modal-backdrop {
    background: var(--iris-color-overlay);
  }

  .modal-card {
    background: var(--iris-color-bg-elevated);
    border: 1px solid var(--iris-color-border);
    border-radius: var(--iris-radius-lg);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
    animation: modal-in 150ms ease;
  }

  @keyframes modal-in {
    from {
      opacity: 0;
      transform: translateY(-8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .modal-sm {
    max-width: 384px;
  }
  .modal-md {
    max-width: 448px;
  }
  .modal-lg {
    max-width: 512px;
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 24px;
    border-bottom: 1px solid var(--iris-color-border);
  }

  .modal-title {
    color: var(--iris-color-text);
  }

  .modal-close-btn {
    padding: 4px;
    border-radius: var(--iris-radius-sm);
    color: var(--iris-color-text-faint);
    transition: all var(--iris-transition-fast);
    background: transparent;
    border: none;
    cursor: pointer;
  }
  .modal-close-btn:hover {
    color: var(--iris-color-text);
    background: var(--iris-color-bg-surface);
  }

  .modal-body {
    padding: 24px;
  }
</style>
