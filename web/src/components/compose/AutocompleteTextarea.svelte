<script lang="ts">
  import { api } from '../../lib/api';
  import type { AutocompleteSuggestion } from '../../lib/api';
  import AutocompleteDropdown from './AutocompleteDropdown.svelte';

  let {
    value = $bindable(''),
    threadId = null,
    composeMode = 'new',
    placeholder = '',
    rows = 8,
    oninput,
  }: {
    value: string;
    threadId?: string | null;
    composeMode?: string;
    placeholder?: string;
    rows?: number;
    oninput?: () => void;
  } = $props();

  let textareaEl: HTMLTextAreaElement | undefined = $state();
  let wrapperEl: HTMLDivElement | undefined = $state();
  let suggestions = $state<AutocompleteSuggestion[]>([]);
  let selectedIndex = $state(0);
  let showDropdown = $state(false);
  let dropdownTop = $state(0);
  let dropdownLeft = $state(0);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let abortController: AbortController | null = null;

  // Minimum characters since last space/punctuation to trigger
  const MIN_CHARS_TRIGGER = 10;
  const DEBOUNCE_MS = 300;

  function shouldTrigger(text: string, cursorPos: number): boolean {
    if (cursorPos === 0) return false;
    const textUpToCursor = text.slice(0, cursorPos);
    if (textUpToCursor.trim().length < MIN_CHARS_TRIGGER) return false;

    // Count chars since last space or punctuation
    const lastBreak = Math.max(
      textUpToCursor.lastIndexOf(' '),
      textUpToCursor.lastIndexOf('.'),
      textUpToCursor.lastIndexOf(','),
      textUpToCursor.lastIndexOf('!'),
      textUpToCursor.lastIndexOf('?'),
      textUpToCursor.lastIndexOf('\n'),
    );

    // If there's a recent break within 3 chars, use total length check instead
    if (lastBreak >= 0 && cursorPos - lastBreak <= 3) {
      return textUpToCursor.trim().length >= MIN_CHARS_TRIGGER;
    }

    return true;
  }

  function updateDropdownPosition() {
    if (!textareaEl || !wrapperEl) return;
    const taRect = textareaEl.getBoundingClientRect();
    const wrapperRect = wrapperEl.getBoundingClientRect();

    // Approximate cursor position from textarea
    // Place dropdown below textarea with a small offset
    dropdownTop = taRect.bottom - wrapperRect.top + 4;
    dropdownLeft = 0;
  }

  async function fetchSuggestions() {
    if (!textareaEl) return;

    const cursorPos = textareaEl.selectionStart;
    const text = value;

    if (!shouldTrigger(text, cursorPos)) {
      dismissSuggestions();
      return;
    }

    // Cancel any in-flight request
    if (abortController) {
      abortController.abort();
    }
    abortController = new AbortController();

    try {
      const partialText = text.slice(0, cursorPos);
      const result = await api.ai.autocomplete(
        threadId,
        partialText,
        cursorPos,
        composeMode,
      );

      if (result.suggestions.length > 0) {
        suggestions = result.suggestions;
        selectedIndex = 0;
        updateDropdownPosition();
        showDropdown = true;
      } else {
        dismissSuggestions();
      }
    } catch {
      // Silently fail — autocomplete is a nice-to-have
      dismissSuggestions();
    }
  }

  function handleInput() {
    // Forward the event
    oninput?.();

    // Debounce autocomplete requests
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(fetchSuggestions, DEBOUNCE_MS);
  }

  function acceptSuggestion(suggestion: AutocompleteSuggestion) {
    if (!textareaEl) return;

    const cursorPos = textareaEl.selectionStart;
    const before = value.slice(0, cursorPos);
    const after = value.slice(cursorPos);

    // Insert the completion text at cursor
    value = before + suggestion.text + after;

    // Move cursor to end of inserted text
    const newPos = cursorPos + suggestion.text.length;
    requestAnimationFrame(() => {
      if (textareaEl) {
        textareaEl.selectionStart = newPos;
        textareaEl.selectionEnd = newPos;
        textareaEl.focus();
      }
    });

    dismissSuggestions();
    oninput?.();
  }

  function dismissSuggestions() {
    suggestions = [];
    showDropdown = false;
    selectedIndex = 0;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!showDropdown || suggestions.length === 0) return;

    switch (e.key) {
      case 'Tab':
      case 'Enter':
        if (showDropdown && suggestions.length > 0) {
          e.preventDefault();
          acceptSuggestion(suggestions[selectedIndex]);
        }
        break;
      case 'Escape':
        e.preventDefault();
        dismissSuggestions();
        break;
      case 'ArrowDown':
        e.preventDefault();
        selectedIndex = (selectedIndex + 1) % suggestions.length;
        break;
      case 'ArrowUp':
        e.preventDefault();
        selectedIndex = (selectedIndex - 1 + suggestions.length) % suggestions.length;
        break;
    }
  }

  function handleBlur() {
    // Small delay to allow click events on suggestions to fire
    setTimeout(() => {
      dismissSuggestions();
    }, 200);
  }

  // Cleanup on unmount
  $effect(() => {
    return () => {
      if (debounceTimer) clearTimeout(debounceTimer);
      if (abortController) abortController.abort();
    };
  });
</script>

<div class="autocomplete-wrapper" bind:this={wrapperEl}>
  <textarea
    bind:this={textareaEl}
    bind:value
    oninput={handleInput}
    onkeydown={handleKeydown}
    onblur={handleBlur}
    class="autocomplete-textarea"
    {placeholder}
    {rows}
  ></textarea>
  <AutocompleteDropdown
    {suggestions}
    {selectedIndex}
    visible={showDropdown}
    top={dropdownTop}
    left={dropdownLeft}
    onaccept={acceptSuggestion}
    ondismiss={dismissSuggestions}
  />
</div>

<style>
  .autocomplete-wrapper {
    position: relative;
    width: 100%;
  }

  .autocomplete-textarea {
    width: 100%;
    min-height: 200px;
    font-size: 0.875rem;
    line-height: 1.625;
    font-family: var(--iris-font-family);
    background: transparent;
    color: var(--iris-color-text);
    border: none;
    outline: none;
    resize: vertical;
    padding: 0;
  }
</style>
