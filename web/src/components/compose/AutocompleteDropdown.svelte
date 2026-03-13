<script lang="ts">
  import type { AutocompleteSuggestion } from '../../lib/api';

  let {
    suggestions,
    selectedIndex = 0,
    visible = false,
    top = 0,
    left = 0,
    onaccept,
    ondismiss,
  }: {
    suggestions: AutocompleteSuggestion[];
    selectedIndex: number;
    visible: boolean;
    top: number;
    left: number;
    onaccept: (suggestion: AutocompleteSuggestion) => void;
    ondismiss: () => void;
  } = $props();

  function handleClick(suggestion: AutocompleteSuggestion) {
    onaccept(suggestion);
  }
</script>

{#if visible && suggestions.length > 0}
  <div
    class="autocomplete-dropdown"
    style="top: {top}px; left: {left}px;"
    role="listbox"
    aria-label="Autocomplete suggestions"
  >
    {#each suggestions as suggestion, i}
      <button
        class="autocomplete-item"
        class:selected={i === selectedIndex}
        role="option"
        aria-selected={i === selectedIndex}
        onmousedown|preventDefault={() => handleClick(suggestion)}
      >
        <span class="autocomplete-text">{suggestion.text}</span>
        <span class="autocomplete-hint">
          {#if i === selectedIndex}Tab{/if}
        </span>
      </button>
    {/each}
    <div class="autocomplete-footer">
      <span class="autocomplete-footer-text">
        <kbd>Tab</kbd> accept &middot; <kbd>Esc</kbd> dismiss &middot; <kbd>&uarr;&darr;</kbd> navigate
      </span>
    </div>
  </div>
{/if}

<style>
  .autocomplete-dropdown {
    position: absolute;
    z-index: 100;
    min-width: 280px;
    max-width: 480px;
    border-radius: var(--iris-radius-md);
    border: 1px solid var(--iris-color-border);
    background: var(--iris-color-bg-elevated);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    padding: calc(var(--iris-spacing-base) * 1) 0;
    opacity: 1;
    transition: opacity var(--iris-transition-fast);
  }

  .autocomplete-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: calc(var(--iris-spacing-base) * 2) calc(var(--iris-spacing-base) * 3);
    border: none;
    background: transparent;
    color: var(--iris-color-text-muted);
    font-size: 0.8125rem;
    font-family: var(--iris-font-family);
    text-align: left;
    cursor: pointer;
    transition: background var(--iris-transition-fast), color var(--iris-transition-fast);
  }

  .autocomplete-item:hover,
  .autocomplete-item.selected {
    background: color-mix(in srgb, var(--iris-color-primary) 12%, transparent);
    color: var(--iris-color-text);
  }

  .autocomplete-text {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .autocomplete-hint {
    flex-shrink: 0;
    margin-left: calc(var(--iris-spacing-base) * 2);
    font-size: 0.6875rem;
    color: var(--iris-color-text-faint);
    font-family: var(--iris-font-mono);
  }

  .autocomplete-footer {
    padding: calc(var(--iris-spacing-base) * 1) calc(var(--iris-spacing-base) * 3);
    border-top: 1px solid var(--iris-color-border-subtle);
    margin-top: calc(var(--iris-spacing-base) * 1);
  }

  .autocomplete-footer-text {
    font-size: 0.625rem;
    color: var(--iris-color-text-faint);
  }

  .autocomplete-footer-text kbd {
    font-family: var(--iris-font-mono);
    font-size: 0.5625rem;
    padding: 1px 3px;
    border-radius: var(--iris-radius-sm);
    border: 1px solid var(--iris-color-border-subtle);
    background: var(--iris-color-bg-surface);
  }
</style>
