<script lang="ts">
  import { api } from '../../lib/api';

  interface CcSuggestion {
    email: string;
    name: string | null;
    reason: string;
    confidence: number;
    type: 'cc' | 'bcc';
  }

  let {
    to,
    cc,
    subject,
    bodyPreview,
    threadId,
    onaddcc,
    onaddbcc,
  }: {
    to: string;
    cc: string;
    subject: string;
    bodyPreview: string;
    threadId?: string;
    onaddcc: (email: string) => void;
    onaddbcc: (email: string) => void;
  } = $props();

  let suggestions = $state<CcSuggestion[]>([]);
  let dismissed = $state<Set<string>>(new Set());
  let loading = $state(false);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  // Split comma-separated addresses into an array
  function splitAddresses(input: string): string[] {
    return input
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);
  }

  // Fetch suggestions with debounce
  async function fetchSuggestions() {
    const toList = splitAddresses(to);
    const ccList = splitAddresses(cc);

    // Need at least one recipient or a thread context
    if (toList.length === 0 && !threadId) {
      suggestions = [];
      return;
    }

    loading = true;
    try {
      const res = await (api as any).ai.suggestCc({
        thread_id: threadId,
        to: toList,
        cc: ccList,
        subject,
        body_preview: bodyPreview.slice(0, 500),
      });
      suggestions = (res.suggestions || []).filter(
        (s: CcSuggestion) => !dismissed.has(s.email)
      );
    } catch {
      // Silent fail — suggestions are non-critical
      suggestions = [];
    } finally {
      loading = false;
    }
  }

  function scheduleFetch() {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(fetchSuggestions, 500);
  }

  function handleClick(suggestion: CcSuggestion, event: MouseEvent) {
    if (event.shiftKey) {
      onaddbcc(suggestion.email);
    } else {
      if (suggestion.type === 'bcc') {
        onaddbcc(suggestion.email);
      } else {
        onaddcc(suggestion.email);
      }
    }
    dismiss(suggestion.email);
  }

  function dismiss(email: string) {
    dismissed = new Set([...dismissed, email]);
    suggestions = suggestions.filter((s) => s.email !== email);
  }

  // Watch recipients and trigger fetch
  $effect(() => {
    // Track reactive dependencies
    to;
    cc;
    scheduleFetch();

    return () => {
      if (debounceTimer) clearTimeout(debounceTimer);
    };
  });

  let visibleSuggestions = $derived(suggestions.slice(0, 3));
</script>

{#if visibleSuggestions.length > 0}
  <div class="cc-suggestions" role="region" aria-label="CC suggestions">
    <span class="cc-suggestions-label">Suggested:</span>
    {#each visibleSuggestions as suggestion (suggestion.email)}
      <span class="cc-chip" title={suggestion.reason}>
        <button
          class="cc-chip-add"
          onclick={(e) => handleClick(suggestion, e)}
          title="{suggestion.type === 'bcc' ? 'Add to BCC' : 'Add to CC'} (Shift+click for BCC)"
        >
          {suggestion.name || suggestion.email}
          {#if suggestion.type === 'bcc'}
            <span class="cc-chip-type">BCC</span>
          {/if}
        </button>
        <button
          class="cc-chip-dismiss"
          onclick={() => dismiss(suggestion.email)}
          title="Dismiss"
          aria-label="Dismiss suggestion for {suggestion.name || suggestion.email}"
        >
          &times;
        </button>
      </span>
    {/each}
  </div>
{/if}

<style>
  .cc-suggestions {
    display: flex;
    align-items: center;
    gap: calc(var(--iris-spacing-base) * 2);
    padding: calc(var(--iris-spacing-base) * 1.5) calc(var(--iris-spacing-base) * 2);
    border-top: 1px solid var(--iris-color-border-subtle);
    flex-wrap: wrap;
  }

  .cc-suggestions-label {
    font-size: 0.75rem;
    color: var(--iris-color-text-faint);
    white-space: nowrap;
  }

  .cc-chip {
    display: inline-flex;
    align-items: center;
    border-radius: var(--iris-radius-md);
    border: 1px solid var(--iris-color-border);
    background: var(--iris-color-bg-elevated);
    overflow: hidden;
    transition: border-color var(--iris-transition-fast);
  }

  .cc-chip:hover {
    border-color: var(--iris-color-primary);
  }

  .cc-chip-add {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--iris-spacing-base) * 1);
    padding: calc(var(--iris-spacing-base) * 1) calc(var(--iris-spacing-base) * 2);
    font-size: 0.75rem;
    color: var(--iris-color-text-muted);
    background: transparent;
    border: none;
    cursor: pointer;
    transition: color var(--iris-transition-fast), background var(--iris-transition-fast);
    white-space: nowrap;
  }

  .cc-chip-add:hover {
    color: var(--iris-color-text);
    background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent);
  }

  .cc-chip-type {
    font-size: 0.625rem;
    padding: 1px calc(var(--iris-spacing-base) * 1);
    border-radius: var(--iris-radius-sm);
    background: color-mix(in srgb, var(--iris-color-text-faint) 15%, transparent);
    color: var(--iris-color-text-faint);
    text-transform: uppercase;
    font-weight: 600;
  }

  .cc-chip-dismiss {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 100%;
    font-size: 0.875rem;
    color: var(--iris-color-text-faint);
    background: transparent;
    border: none;
    border-left: 1px solid var(--iris-color-border-subtle);
    cursor: pointer;
    transition: color var(--iris-transition-fast), background var(--iris-transition-fast);
  }

  .cc-chip-dismiss:hover {
    color: var(--iris-color-error);
    background: color-mix(in srgb, var(--iris-color-error) 10%, transparent);
  }
</style>
