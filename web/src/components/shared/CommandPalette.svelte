<script lang="ts">
  import { paletteOpen, searchCommands, type Command } from '$lib/commands';
  import { irisFade, irisScale } from '$lib/transitions';

  let query = $state('');
  let focusedIdx = $state(0);
  let inputEl: HTMLInputElement | undefined = $state();

  let isOpen = $state(false);

  // Subscribe to store
  const unsubPalette = paletteOpen.subscribe(v => {
    isOpen = v;
  });

  $effect(() => {
    return () => unsubPalette();
  });

  // Reset and focus when opened
  $effect(() => {
    if (isOpen) {
      query = '';
      focusedIdx = 0;
      // Defer focus to next tick so element is rendered
      setTimeout(() => inputEl?.focus(), 10);
    }
  });

  let results = $derived(searchCommands(query));

  // Group results by category
  let grouped = $derived(() => {
    const map = new Map<string, Command[]>();
    for (const cmd of results) {
      const list = map.get(cmd.category) ?? [];
      list.push(cmd);
      map.set(cmd.category, list);
    }
    return map;
  });

  // Flat ordered list for arrow key navigation
  let flatList = $derived(results);

  function close() {
    paletteOpen.set(false);
  }

  function execute(cmd: Command) {
    close();
    cmd.action();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      focusedIdx = Math.min(focusedIdx + 1, flatList.length - 1);
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      focusedIdx = Math.max(focusedIdx - 1, 0);
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      const cmd = flatList[focusedIdx];
      if (cmd) execute(cmd);
      return;
    }
  }

  function handleBackdrop(e: MouseEvent) {
    if (e.target === e.currentTarget) close();
  }

  // Reset focused index when results change
  $effect(() => {
    // Access results to create dependency
    const _ = results;
    focusedIdx = 0;
  });
</script>

{#if isOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div
    class="palette-backdrop"
    onclick={handleBackdrop}
    transition:irisFade
    role="dialog"
    aria-modal="true"
    aria-label="Command palette"
  >
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <!-- svelte-ignore a11y_role_has_required_aria_props -->
    <div
      class="palette-card"
      transition:irisScale
      role="combobox"
      aria-expanded="true"
      aria-haspopup="listbox"
      aria-owns="palette-listbox"
      aria-label="Command palette search"
      onkeydown={handleKeydown}
      tabindex="-1"
    >
      <!-- Search input -->
      <div class="palette-search-row">
        <svg class="palette-search-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="11" cy="11" r="8"/>
          <line x1="21" y1="21" x2="16.65" y2="16.65"/>
        </svg>
        <input
          bind:this={inputEl}
          bind:value={query}
          class="palette-search-input"
          type="text"
          placeholder="Search commands..."
          autocomplete="off"
          spellcheck="false"
          aria-label="Search commands"
          aria-autocomplete="list"
          aria-controls="palette-listbox"
        />
        <kbd class="palette-kbd">esc</kbd>
      </div>

      <!-- Results list -->
      {#if flatList.length > 0}
        <ul class="palette-list" id="palette-listbox" role="listbox">
          {#each [...grouped()] as [category, cmds]}
            <li class="palette-category-header" role="presentation">{category}</li>
            {#each cmds as cmd}
              {@const idx = flatList.indexOf(cmd)}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <li
                class="palette-item {idx === focusedIdx ? 'palette-item--focused' : ''}"
                role="option"
                aria-selected={idx === focusedIdx}
                onclick={() => execute(cmd)}
                onmouseenter={() => { focusedIdx = idx; }}
              >
                <span class="palette-item-label">{cmd.label}</span>
                {#if cmd.shortcut}
                  <span class="palette-item-shortcut">{cmd.shortcut}</span>
                {/if}
              </li>
            {/each}
          {/each}
        </ul>
      {:else}
        <div class="palette-empty">No commands found</div>
      {/if}

      <!-- Footer -->
      <div class="palette-footer">
        <span><kbd class="palette-kbd-hint">↑</kbd><kbd class="palette-kbd-hint">↓</kbd> navigate</span>
        <span><kbd class="palette-kbd-hint">↵</kbd> select</span>
        <span><kbd class="palette-kbd-hint">esc</kbd> close</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .palette-backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 20vh;
    background: var(--iris-color-overlay);
  }

  .palette-card {
    width: 100%;
    max-width: 560px;
    margin: 0 16px;
    background: var(--iris-color-bg-elevated);
    border: 1px solid var(--iris-color-border);
    border-radius: var(--iris-radius-lg);
    box-shadow: var(--iris-shadow-lg);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .palette-search-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--iris-color-border-subtle);
  }

  .palette-search-icon {
    flex-shrink: 0;
    color: var(--iris-color-text-faint);
  }

  .palette-search-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    font-family: var(--iris-font-family);
    font-size: 15px;
    color: var(--iris-color-text);
    caret-color: var(--iris-color-primary);
  }

  .palette-search-input::placeholder {
    color: var(--iris-color-text-faint);
  }

  .palette-kbd {
    flex-shrink: 0;
    font-family: var(--iris-font-mono);
    font-size: 11px;
    color: var(--iris-color-text-faint);
    background: var(--iris-color-bg-surface);
    border: 1px solid var(--iris-color-border-subtle);
    border-radius: var(--iris-radius-sm);
    padding: 2px 6px;
    line-height: 1.4;
  }

  .palette-list {
    list-style: none;
    margin: 0;
    padding: 8px 0;
    max-height: 320px;
    overflow-y: auto;
  }

  .palette-category-header {
    padding: 8px 16px 4px;
    font-family: var(--iris-font-family);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--iris-color-text-faint);
  }

  .palette-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px;
    cursor: pointer;
    transition: background var(--iris-transition-fast);
  }

  .palette-item--focused {
    background: var(--iris-color-bg-hover);
  }

  .palette-item-label {
    font-family: var(--iris-font-family);
    font-size: 14px;
    color: var(--iris-color-text);
  }

  .palette-item-shortcut {
    font-family: var(--iris-font-mono);
    font-size: 11px;
    color: var(--iris-color-text-faint);
    background: var(--iris-color-bg-surface);
    border: 1px solid var(--iris-color-border-subtle);
    border-radius: var(--iris-radius-sm);
    padding: 2px 6px;
    flex-shrink: 0;
  }

  .palette-empty {
    padding: 24px 16px;
    text-align: center;
    font-family: var(--iris-font-family);
    font-size: 14px;
    color: var(--iris-color-text-faint);
  }

  .palette-footer {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 8px 16px;
    border-top: 1px solid var(--iris-color-border-subtle);
    font-family: var(--iris-font-family);
    font-size: 12px;
    color: var(--iris-color-text-faint);
  }

  .palette-kbd-hint {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-family: var(--iris-font-mono);
    font-size: 11px;
    color: var(--iris-color-text-faint);
    background: var(--iris-color-bg-surface);
    border: 1px solid var(--iris-color-border-subtle);
    border-radius: var(--iris-radius-sm);
    padding: 1px 4px;
    margin-right: 3px;
    line-height: 1.4;
  }
</style>
