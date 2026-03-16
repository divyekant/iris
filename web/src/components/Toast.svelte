<script lang="ts">
  import { feedback } from '$lib/feedback';
  import { irisSlide } from '$lib/transitions';
</script>

<div class="toast-stack" aria-live="polite" aria-atomic="false">
  {#each $feedback as item (item.id)}
    <div
      class="toast-card toast-{item.type}"
      role="status"
      transition:irisSlide
    >
      <span class="toast-message">{item.message}</span>
      <div class="toast-actions">
        {#if item.undoFn}
          <button class="toast-undo" onclick={() => feedback.undo(item.id)}>Undo</button>
        {/if}
        <button class="toast-dismiss" aria-label="Dismiss" onclick={() => feedback.dismiss(item.id)}>✕</button>
      </div>
    </div>
  {/each}
</div>

<style>
  .toast-stack {
    position: fixed;
    bottom: 1rem;
    right: 1rem;
    z-index: 9000;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    align-items: flex-end;
    pointer-events: none;
  }

  .toast-card {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.625rem 1rem;
    border-radius: 8px;
    border: 1px solid var(--iris-color-border);
    background: var(--iris-color-bg-elevated);
    color: var(--iris-color-text);
    font-size: 0.875rem;
    max-width: 22rem;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.25);
    pointer-events: all;
  }

  .toast-success {
    border-left: 3px solid #16A34A;
  }

  .toast-error {
    border-left: 3px solid #DC2626;
  }

  .toast-info {
    border-left: 3px solid var(--iris-color-primary);
  }

  .toast-message {
    flex: 1;
  }

  .toast-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-shrink: 0;
  }

  .toast-undo {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--iris-color-primary);
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    text-decoration: underline;
  }

  .toast-undo:hover {
    opacity: 0.8;
  }

  .toast-dismiss {
    font-size: 0.75rem;
    color: var(--iris-color-text-muted);
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    line-height: 1;
  }

  .toast-dismiss:hover {
    color: var(--iris-color-text);
  }
</style>
