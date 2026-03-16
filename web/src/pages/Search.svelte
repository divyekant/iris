<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { push, querystring } from 'svelte-spa-router';
  import { ArrowLeft, Paperclip, Search, Bookmark, X, HelpCircle, Plus } from 'lucide-svelte';
  import EmptyState from '../components/EmptyState.svelte';

  let searchQuery = $state('');
  let results = $state<any[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let searched = $state(false);
  let parsedOperators = $state<{ key: string; value: string }[]>([]);

  // Filters
  let semantic = $state(false);
  let temporal = $state(false);
  let temporalRange = $state<{ description: string; start_date: string; end_date: string; confidence: number } | null>(null);
  let temporalResults = $state<any[]>([]);
  let temporalTotal = $state(0);

  // Saved searches
  let savedSearches = $state<{ id: string; name: string; query: string; account_id: string | null; created_at: number }[]>([]);
  let showSaveDialog = $state(false);
  let saveName = $state('');
  let showOperatorHelp = $state(false);

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  // Operator → CSS class mapping (colors defined in <style> block via design tokens)
  const operatorClassMap: Record<string, string> = {
    from: 'operator-from',
    to: 'operator-to',
    subject: 'operator-subject',
    is: 'operator-is',
    has: 'operator-has',
    before: 'operator-before',
    after: 'operator-after',
    category: 'operator-category',
  };

  const operatorHelp = [
    { op: 'from:', desc: 'Sender email or name', example: 'from:sarah@acme.com' },
    { op: 'to:', desc: 'Recipient email or name', example: 'to:team@company.com' },
    { op: 'subject:', desc: 'Subject line text', example: 'subject:"quarterly report"' },
    { op: 'is:', desc: 'Message state', example: 'is:unread, is:read, is:starred' },
    { op: 'has:', desc: 'Message attributes', example: 'has:attachment' },
    { op: 'before:', desc: 'Date upper bound', example: 'before:2026-03-01' },
    { op: 'after:', desc: 'Date lower bound', example: 'after:2026-01-01' },
    { op: 'category:', desc: 'AI category', example: 'category:promotions' },
  ];

  async function doSearch() {
    if (!searchQuery.trim()) {
      results = [];
      total = 0;
      searched = false;
      parsedOperators = [];
      temporalRange = null;
      temporalResults = [];
      temporalTotal = 0;
      return;
    }
    loading = true;
    searched = true;
    try {
      if (temporal) {
        const res = await api.temporal.search(searchQuery);
        temporalRange = res.resolved_range;
        temporalResults = res.messages;
        temporalTotal = res.total;
        results = [];
        total = 0;
        parsedOperators = [];
      } else {
        temporalRange = null;
        temporalResults = [];
        temporalTotal = 0;
        const res = await api.search({
          q: searchQuery,
          semantic: semantic || undefined,
        });
        results = res.results;
        total = res.total;
        parsedOperators = res.parsed_operators || [];
      }
    } catch {
      results = [];
      total = 0;
      parsedOperators = [];
      temporalRange = null;
      temporalResults = [];
      temporalTotal = 0;
    } finally {
      loading = false;
    }
  }

  function onInput() {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(doSearch, 300);
  }

  function handleResultClick(result: any) {
    const threadId = result.thread_id || result.id;
    push(`/thread/${encodeURIComponent(threadId)}`);
  }

  function formatDate(timestamp: number): string {
    const d = new Date(timestamp * 1000);
    return d.toLocaleDateString([], { month: 'short', day: 'numeric', year: 'numeric' });
  }

  function toggleSemantic() {
    semantic = !semantic;
    if (semantic) temporal = false;
    doSearch();
  }

  function toggleTemporal() {
    temporal = !temporal;
    if (temporal) semantic = false;
    doSearch();
  }

  function removeOperator(index: number) {
    const op = parsedOperators[index];
    // Remove the operator from the query string
    const opStr = op.value.includes(' ') ? `${op.key}:"${op.value}"` : `${op.key}:${op.value}`;
    searchQuery = searchQuery.replace(opStr, '').replace(/\s+/g, ' ').trim();
    doSearch();
  }

  function insertOperator(op: string) {
    const current = searchQuery.trim();
    searchQuery = current ? `${current} ${op}` : op;
    document.getElementById('search-input')?.focus();
    doSearch();
  }

  function insertSuggestion(suggestion: string) {
    searchQuery = suggestion;
    doSearch();
  }

  // Saved search management
  async function loadSavedSearches() {
    try {
      savedSearches = await api.savedSearches.list();
    } catch {
      savedSearches = [];
    }
  }

  async function saveCurrentSearch() {
    const name = saveName.trim();
    if (!name || !searchQuery.trim()) return;
    try {
      await api.savedSearches.create({ name, query: searchQuery });
      saveName = '';
      showSaveDialog = false;
      await loadSavedSearches();
    } catch {
      // ignore
    }
  }

  async function deleteSavedSearch(id: string) {
    try {
      await api.savedSearches.delete(id);
      savedSearches = savedSearches.filter(s => s.id !== id);
    } catch {
      // ignore
    }
  }

  function applySavedSearch(query: string) {
    searchQuery = query;
    doSearch();
  }

  function handleGlobalKeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      document.getElementById('search-input')?.focus();
    }
    if (e.key === '/' && !(e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement)) {
      e.preventDefault();
      document.getElementById('search-input')?.focus();
    }
    if (e.key === 'Escape') {
      showSaveDialog = false;
      showOperatorHelp = false;
    }
  }

  onMount(() => {
    loadSavedSearches();

    // Check for query param from URL
    const params = new URLSearchParams($querystring || '');
    const q = params.get('q');
    if (q) {
      searchQuery = q;
      doSearch();
    }
  });
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<div class="h-full flex">
  <!-- Saved Searches Sidebar -->
  {#if savedSearches.length > 0}
    <div class="w-56 flex-shrink-0 border-r overflow-y-auto" style="border-color: var(--iris-color-border-subtle); background: var(--iris-color-bg-surface);">
      <div class="px-3 py-2.5 flex items-center gap-1.5" style="color: var(--iris-color-text-muted);">
        <Bookmark size={14} />
        <span class="text-xs font-medium uppercase tracking-wide">Saved Searches</span>
      </div>
      <div class="px-1.5">
        {#each savedSearches as saved (saved.id)}
          <div class="group flex items-center gap-1">
            <button
              class="flex-1 text-left px-2 py-1.5 text-sm rounded transition-colors truncate saved-search-item"
              style="color: var(--iris-color-text);"
              onclick={() => applySavedSearch(saved.query)}
              title={saved.query}
            >
              {saved.name}
            </button>
            <button
              class="opacity-0 group-hover:opacity-100 p-0.5 rounded transition-opacity"
              style="color: var(--iris-color-text-faint);"
              onclick={() => deleteSavedSearch(saved.id)}
              title="Remove saved search"
            ><X size={12} /></button>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Main Search Area -->
  <div class="flex-1 flex flex-col min-w-0">
    <!-- Search header -->
    <div class="px-4 py-3 border-b" style="border-color: var(--iris-color-border);">
      <div class="flex items-center gap-3">
        <button
          class="p-1 transition-colors"
          style="color: var(--iris-color-text-faint);"
          onclick={() => push('/')}
          title="Back to inbox"
        ><ArrowLeft size={16} /></button>
        <div class="flex-1 relative">
          <input
            type="text"
            bind:value={searchQuery}
            oninput={onInput}
            placeholder="Search emails... (use operators like from: is: has:)"
            id="search-input"
            class="w-full px-3 py-2 rounded-lg text-sm focus:outline-none focus:ring-2"
            style="background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
            autofocus
          />
        </div>
        <!-- Save Search Button -->
        {#if searchQuery.trim()}
          <button
            class="px-2.5 py-1.5 text-xs rounded-md border transition-colors flex items-center gap-1"
            style="color: var(--iris-color-text-muted); border-color: var(--iris-color-border);"
            onclick={() => { showSaveDialog = !showSaveDialog; showOperatorHelp = false; }}
            title="Save this search"
          >
            <Bookmark size={12} />
            <span>Save</span>
          </button>
        {/if}
        <!-- Help Button -->
        <button
          class="p-1.5 rounded transition-colors"
          style="color: var(--iris-color-text-faint);"
          onclick={() => { showOperatorHelp = !showOperatorHelp; showSaveDialog = false; }}
          title="Search operators help"
        ><HelpCircle size={16} /></button>
      </div>

      <!-- Save Search Dialog -->
      {#if showSaveDialog}
        <div class="mt-2 p-3 rounded-lg border" style="background: var(--iris-color-bg-elevated); border-color: var(--iris-color-border);">
          <div class="flex items-center gap-2">
            <input
              type="text"
              bind:value={saveName}
              placeholder="Name this search..."
              class="flex-1 px-2 py-1.5 rounded text-sm focus:outline-none focus:ring-1"
              style="background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
              onkeydown={(e) => { if (e.key === 'Enter') saveCurrentSearch(); }}
              autofocus
            />
            <button
              class="px-3 py-1.5 rounded text-xs font-medium transition-colors"
              style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
              onclick={saveCurrentSearch}
              disabled={!saveName.trim()}
            >Save</button>
            <button
              class="p-1 rounded transition-colors"
              style="color: var(--iris-color-text-faint);"
              onclick={() => { showSaveDialog = false; }}
            ><X size={14} /></button>
          </div>
          <div class="mt-1.5 text-xs" style="color: var(--iris-color-text-faint);">
            Query: {searchQuery}
          </div>
        </div>
      {/if}

      <!-- Parsed Operator Chips -->
      {#if parsedOperators.length > 0 || semantic || temporal}
        <div class="flex flex-wrap gap-1.5 mt-2">
          {#if semantic}
            <button
              class="px-2.5 py-1 text-xs rounded-full border transition-colors flex items-center gap-1"
              style="background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent); color: var(--iris-color-primary); border-color: var(--iris-color-primary);"
              onclick={toggleSemantic}
            >
              Semantic
              <X size={10} />
            </button>
          {/if}
          {#if temporal}
            <button
              class="px-2.5 py-1 text-xs rounded-full border transition-colors flex items-center gap-1"
              style="background: color-mix(in srgb, var(--iris-color-warning) 10%, transparent); color: var(--iris-color-warning); border-color: var(--iris-color-warning);"
              onclick={toggleTemporal}
            >
              Temporal
              <X size={10} />
            </button>
            {#if temporalRange}
              <span
                class="px-2.5 py-1 text-xs rounded-full border"
                style="background: color-mix(in srgb, var(--iris-color-info) 8%, transparent); color: var(--iris-color-info); border-color: color-mix(in srgb, var(--iris-color-info) 25%, transparent);"
                title="{temporalRange.start_date} to {temporalRange.end_date} (confidence: {Math.round(temporalRange.confidence * 100)}%)"
              >{temporalRange.description}</span>
            {/if}
          {/if}
          {#each parsedOperators as op, i}
            <button
              class="px-2.5 py-1 text-xs rounded-full border transition-colors flex items-center gap-1 operator-chip {operatorClassMap[op.key] || 'operator-default'}"
              onclick={() => removeOperator(i)}
              title="Click to remove"
            >
              <span class="font-medium">{op.key}:</span>{op.value}
              <X size={10} />
            </button>
          {/each}
        </div>
      {:else if !searched}
        <!-- Quick filter chips when no search yet -->
        <div class="flex gap-2 mt-2">
          <button
            class="px-3 py-1 text-xs rounded-full border transition-colors"
            style={semantic
              ? 'background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent); color: var(--iris-color-primary); border-color: var(--iris-color-primary);'
              : 'background: transparent; color: var(--iris-color-text-muted); border-color: var(--iris-color-border);'}
            onclick={toggleSemantic}
            title="Search by meaning, not just keywords"
          >Semantic</button>
          <button
            class="px-3 py-1 text-xs rounded-full border transition-colors"
            style={temporal
              ? 'background: color-mix(in srgb, var(--iris-color-warning) 10%, transparent); color: var(--iris-color-warning); border-color: var(--iris-color-warning);'
              : 'background: transparent; color: var(--iris-color-text-muted); border-color: var(--iris-color-border);'}
            onclick={toggleTemporal}
            title="Search by time reference (e.g., 'around when we launched v2')"
          >Temporal</button>
        </div>
      {/if}
    </div>

    <!-- Operator Help Panel -->
    {#if showOperatorHelp}
      <div class="px-4 py-3 border-b" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface);">
        <div class="flex items-center justify-between mb-2">
          <span class="text-xs font-medium uppercase tracking-wide" style="color: var(--iris-color-text-muted);">Search Operators</span>
          <button
            class="p-0.5 rounded"
            style="color: var(--iris-color-text-faint);"
            onclick={() => { showOperatorHelp = false; }}
          ><X size={14} /></button>
        </div>
        <div class="grid grid-cols-2 gap-x-6 gap-y-1">
          {#each operatorHelp as h}
            <button
              class="text-left flex items-baseline gap-2 py-1 rounded transition-colors operator-help-row"
              onclick={() => insertOperator(h.op)}
            >
              <code class="text-xs font-mono font-semibold {operatorClassMap[h.op.replace(':', '')] || 'operator-default'} operator-help-label">{h.op}</code>
              <span class="text-xs" style="color: var(--iris-color-text-faint);">{h.desc}</span>
            </button>
          {/each}
        </div>
        <div class="mt-2 text-xs" style="color: var(--iris-color-text-faint);">
          Combine operators: <code class="font-mono" style="color: var(--iris-color-text-muted);">from:sarah is:unread has:attachment</code>
        </div>
      </div>
    {/if}

    <!-- Results -->
    <div class="flex-1 overflow-auto">
      {#if loading}
        <div class="flex items-center justify-center py-16">
          <div class="w-8 h-8 border-4 rounded-full animate-spin" style="border-color: color-mix(in srgb, var(--iris-color-primary) 20%, transparent); border-top-color: var(--iris-color-primary);"></div>
        </div>
      {:else if temporal && searched}
        <!-- Temporal search results -->
        {#if temporalResults.length === 0}
          <EmptyState icon="search" title="No temporal results" subtitle="Try a different time reference." />
        {:else}
          <div class="px-4 py-2 text-xs" style="color: var(--iris-color-text-faint);">
            {temporalTotal} result{temporalTotal === 1 ? '' : 's'}
            {#if temporalRange}
              &middot; {temporalRange.start_date} to {temporalRange.end_date}
            {/if}
          </div>
          <div class="divide-y" style="--tw-divide-color: var(--iris-color-border-subtle);">
            {#each temporalResults as result (result.id)}
              <button
                class="w-full text-left px-4 py-3 transition-colors flex items-start gap-3 search-result-row"
                onclick={() => handleResultClick(result)}
              >
                <div class="pt-1.5 w-3 flex-shrink-0">
                  {#if !result.is_read}
                    <div class="w-2.5 h-2.5 rounded-full" style="background: var(--iris-color-unread);"></div>
                  {/if}
                </div>
                <div class="flex-1 min-w-0">
                  <div class="flex items-baseline gap-2">
                    <span class="text-sm truncate {result.is_read ? '' : 'font-semibold'}" style="color: var(--iris-color-text);">
                      {result.from_name || result.from_address || 'Unknown'}
                    </span>
                    <span class="flex-shrink-0 text-xs ml-auto" style="color: var(--iris-color-text-faint);">
                      {#if result.date}
                        {formatDate(result.date)}
                      {/if}
                    </span>
                  </div>
                  <div class="text-sm truncate {result.is_read ? '' : 'font-medium'}" style="color: var(--iris-color-text);">
                    {result.subject || '(no subject)'}
                  </div>
                  {#if result.snippet}
                    <div class="text-xs mt-0.5 line-clamp-2" style="color: var(--iris-color-text-muted);">
                      {result.snippet}
                    </div>
                  {/if}
                </div>
              </button>
            {/each}
          </div>
        {/if}
      {:else if searched && results.length === 0}
        <EmptyState icon="search" title="No results found" subtitle="Try different keywords or remove some filters." />
      {:else if results.length > 0}
        <div class="px-4 py-2 text-xs" style="color: var(--iris-color-text-faint);">
          {total} result{total === 1 ? '' : 's'} for &lsquo;{searchQuery}&rsquo;
        </div>
        <div class="divide-y" style="--tw-divide-color: var(--iris-color-border-subtle);">
          {#each results as result (result.id)}
            <button
              class="w-full text-left px-4 py-3 transition-colors flex items-start gap-3 search-result-row"
              onclick={() => handleResultClick(result)}
            >
              <div class="pt-1.5 w-3 flex-shrink-0">
                {#if !result.is_read}
                  <div class="w-2.5 h-2.5 rounded-full" style="background: var(--iris-color-unread);"></div>
                {/if}
              </div>
              <div class="flex-1 min-w-0">
                <div class="flex items-baseline gap-2">
                  <span class="text-sm truncate {result.is_read ? '' : 'font-semibold'}" style="color: var(--iris-color-text);">
                    {result.from_name || result.from_address || 'Unknown'}
                  </span>
                  <span class="flex-shrink-0 text-xs ml-auto flex items-center gap-1.5" style="color: var(--iris-color-text-faint);">
                    {#if result.has_attachments}
                      <span title="Has attachments"><Paperclip size={12} /></span>
                    {/if}
                    {#if result.date}
                      {formatDate(result.date)}
                    {/if}
                  </span>
                </div>
                <div class="text-sm truncate {result.is_read ? '' : 'font-medium'}" style="color: var(--iris-color-text);">
                  {result.subject || '(no subject)'}
                </div>
                <div class="text-xs mt-0.5 line-clamp-2" style="color: var(--iris-color-text-muted);">
                  {@html result.snippet}
                </div>
              </div>
            </button>
          {/each}
        </div>
      {:else}
        <div class="text-center py-16">
          <p class="text-lg mb-2" style="color: var(--iris-color-text);">Search your emails</p>
          <p class="text-sm mb-6" style="color: var(--iris-color-text-muted);">
            Use operators like <code class="font-mono text-xs px-1 py-0.5 rounded" style="background: var(--iris-color-bg-surface);">from:</code> <code class="font-mono text-xs px-1 py-0.5 rounded" style="background: var(--iris-color-bg-surface);">is:</code> <code class="font-mono text-xs px-1 py-0.5 rounded" style="background: var(--iris-color-bg-surface);">has:</code> or press <kbd class="px-1.5 py-0.5 rounded text-xs" style="background: var(--iris-color-bg-surface); border: 1px solid var(--iris-color-border);">{navigator.platform.includes('Mac') ? '\u2318' : 'Ctrl'}+K</kbd>
          </p>
          <div class="flex flex-wrap justify-center gap-2 max-w-md mx-auto">
            {#each [
              'from:stockx is:unread',
              'has:attachment after:2026-01-01',
              'is:starred subject:invoice',
              'category:promotions',
            ] as suggestion}
              <button
                class="px-3 py-1.5 text-xs rounded-full border transition-colors suggestion-chip"
                style="color: var(--iris-color-text-muted); border-color: var(--iris-color-border);"
                onclick={() => insertSuggestion(suggestion)}
              >{suggestion}</button>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .search-result-row:hover {
    background: var(--iris-color-bg-surface);
  }
  .saved-search-item:hover {
    background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent);
  }
  .suggestion-chip:hover {
    background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent);
    border-color: var(--iris-color-primary);
    color: var(--iris-color-primary);
  }
  .operator-help-row:hover {
    background: color-mix(in srgb, var(--iris-color-primary) 5%, transparent);
  }

  /* Operator chip token-based colors */
  .operator-chip {
    background: color-mix(in srgb, var(--iris-color-text-muted) 10%, transparent);
    color: var(--iris-color-text-muted);
    border-color: var(--iris-color-border);
  }
  .operator-chip.operator-from {
    background: color-mix(in srgb, var(--iris-color-info) 12%, transparent);
    color: var(--iris-color-info);
    border-color: color-mix(in srgb, var(--iris-color-info) 30%, transparent);
  }
  .operator-chip.operator-to {
    background: color-mix(in srgb, var(--iris-color-info) 12%, transparent);
    color: var(--iris-color-info);
    border-color: color-mix(in srgb, var(--iris-color-info) 30%, transparent);
  }
  .operator-chip.operator-subject {
    background: color-mix(in srgb, var(--iris-color-success) 12%, transparent);
    color: var(--iris-color-success);
    border-color: color-mix(in srgb, var(--iris-color-success) 30%, transparent);
  }
  .operator-chip.operator-is {
    background: color-mix(in srgb, var(--iris-color-warning) 12%, transparent);
    color: var(--iris-color-warning);
    border-color: color-mix(in srgb, var(--iris-color-warning) 30%, transparent);
  }
  .operator-chip.operator-has {
    background: color-mix(in srgb, var(--iris-color-error) 12%, transparent);
    color: var(--iris-color-error);
    border-color: color-mix(in srgb, var(--iris-color-error) 30%, transparent);
  }
  .operator-chip.operator-before,
  .operator-chip.operator-after {
    background: color-mix(in srgb, var(--iris-color-info) 12%, transparent);
    color: var(--iris-color-info);
    border-color: color-mix(in srgb, var(--iris-color-info) 30%, transparent);
  }
  .operator-chip.operator-category {
    background: color-mix(in srgb, var(--iris-color-warning) 12%, transparent);
    color: var(--iris-color-warning);
    border-color: color-mix(in srgb, var(--iris-color-warning) 30%, transparent);
  }

  /* Operator help label colors (text color only) */
  .operator-help-label.operator-default { color: var(--iris-color-text-muted); }
  .operator-help-label.operator-from { color: var(--iris-color-info); }
  .operator-help-label.operator-to { color: var(--iris-color-info); }
  .operator-help-label.operator-subject { color: var(--iris-color-success); }
  .operator-help-label.operator-is { color: var(--iris-color-warning); }
  .operator-help-label.operator-has { color: var(--iris-color-error); }
  .operator-help-label.operator-before { color: var(--iris-color-info); }
  .operator-help-label.operator-after { color: var(--iris-color-info); }
  .operator-help-label.operator-category { color: var(--iris-color-warning); }
</style>
