<script lang="ts">
  import { api } from '../lib/api';
  import { push } from 'svelte-spa-router';

  let searchQuery = $state('');
  let results = $state<any[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let searched = $state(false);

  // Filters
  let hasAttachment = $state(false);
  let dateFilter = $state('');
  let semantic = $state(false);

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  function getDateRange(): { after?: number; before?: number } {
    const now = Math.floor(Date.now() / 1000);
    switch (dateFilter) {
      case '7d': return { after: now - 7 * 86400 };
      case '30d': return { after: now - 30 * 86400 };
      case '1y': return { after: now - 365 * 86400 };
      default: return {};
    }
  }

  async function doSearch() {
    if (!searchQuery.trim()) {
      results = [];
      total = 0;
      searched = false;
      return;
    }
    loading = true;
    searched = true;
    try {
      const dateRange = getDateRange();
      const res = await api.search({
        q: searchQuery,
        has_attachment: hasAttachment || undefined,
        semantic: semantic || undefined,
        ...dateRange,
      });
      results = res.results;
      total = res.total;
    } catch {
      results = [];
      total = 0;
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

  function toggleFilter(filter: string) {
    dateFilter = dateFilter === filter ? '' : filter;
    doSearch();
  }

  function toggleAttachment() {
    hasAttachment = !hasAttachment;
    doSearch();
  }

  function toggleSemantic() {
    semantic = !semantic;
    doSearch();
  }
</script>

<div class="h-full flex flex-col">
  <!-- Search header -->
  <div class="px-4 py-3 border-b border-gray-200 dark:border-gray-700">
    <div class="flex items-center gap-3">
      <button
        class="p-1 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 transition-colors"
        onclick={() => push('/')}
        title="Back to inbox"
      >&larr;</button>
      <input
        type="text"
        bind:value={searchQuery}
        oninput={onInput}
        placeholder="Search emails..."
        class="flex-1 px-3 py-2 rounded-lg bg-gray-100 dark:bg-gray-800 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        autofocus
      />
    </div>

    <!-- Filter chips -->
    <div class="flex gap-2 mt-2">
      <button
        class="px-3 py-1 text-xs rounded-full border transition-colors
               {semantic ? 'bg-purple-100 dark:bg-purple-900/40 border-purple-300 dark:border-purple-700 text-purple-700 dark:text-purple-300' : 'border-gray-300 dark:border-gray-600 text-gray-500 hover:bg-gray-100 dark:hover:bg-gray-800'}"
        onclick={toggleSemantic}
        title="Search by meaning, not just keywords"
      >Semantic</button>
      <button
        class="px-3 py-1 text-xs rounded-full border transition-colors
               {hasAttachment ? 'bg-blue-100 dark:bg-blue-900/40 border-blue-300 dark:border-blue-700 text-blue-700 dark:text-blue-300' : 'border-gray-300 dark:border-gray-600 text-gray-500 hover:bg-gray-100 dark:hover:bg-gray-800'}"
        onclick={toggleAttachment}
      >has:attachment</button>
      {#each [{ id: '7d', label: 'Last 7 days' }, { id: '30d', label: 'Last 30 days' }, { id: '1y', label: 'Last year' }] as f}
        <button
          class="px-3 py-1 text-xs rounded-full border transition-colors
                 {dateFilter === f.id ? 'bg-blue-100 dark:bg-blue-900/40 border-blue-300 dark:border-blue-700 text-blue-700 dark:text-blue-300' : 'border-gray-300 dark:border-gray-600 text-gray-500 hover:bg-gray-100 dark:hover:bg-gray-800'}"
          onclick={() => toggleFilter(f.id)}
        >{f.label}</button>
      {/each}
    </div>
  </div>

  <!-- Results -->
  <div class="flex-1 overflow-auto">
    {#if loading}
      <div class="flex items-center justify-center py-16">
        <div class="w-8 h-8 border-4 border-blue-200 border-t-blue-600 rounded-full animate-spin"></div>
      </div>
    {:else if searched && results.length === 0}
      <div class="text-center py-16 text-gray-400 dark:text-gray-500">
        <p class="text-lg mb-2">No results found</p>
        <p class="text-sm">Try different keywords or adjust your filters.</p>
      </div>
    {:else if results.length > 0}
      <div class="px-4 py-2 text-xs text-gray-400 dark:text-gray-500">
        {total} result{total === 1 ? '' : 's'}
      </div>
      <div class="divide-y divide-gray-100 dark:divide-gray-800">
        {#each results as result (result.id)}
          <button
            class="w-full text-left px-4 py-3 hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors flex items-start gap-3"
            onclick={() => handleResultClick(result)}
          >
            <div class="pt-1.5 w-3 flex-shrink-0">
              {#if !result.is_read}
                <div class="w-2.5 h-2.5 rounded-full bg-blue-500"></div>
              {/if}
            </div>
            <div class="flex-1 min-w-0">
              <div class="flex items-baseline gap-2">
                <span class="text-sm truncate {result.is_read ? 'text-gray-700 dark:text-gray-300' : 'font-semibold text-gray-900 dark:text-gray-100'}">
                  {result.from_name || result.from_address || 'Unknown'}
                </span>
                <span class="flex-shrink-0 text-xs text-gray-400 dark:text-gray-500 ml-auto">
                  {#if result.has_attachments}
                    <span class="mr-1.5" title="Has attachments">&#128206;</span>
                  {/if}
                  {#if result.date}
                    {formatDate(result.date)}
                  {/if}
                </span>
              </div>
              <div class="text-sm truncate {result.is_read ? 'text-gray-600 dark:text-gray-400' : 'font-medium text-gray-800 dark:text-gray-200'}">
                {result.subject || '(no subject)'}
              </div>
              <div class="text-xs text-gray-400 dark:text-gray-500 mt-0.5 line-clamp-2">
                {@html result.snippet}
              </div>
            </div>
          </button>
        {/each}
      </div>
    {:else}
      <div class="text-center py-16 text-gray-400 dark:text-gray-500">
        <p class="text-lg mb-2">Search your emails</p>
        <p class="text-sm">Type a keyword to find messages by content, subject, or sender.</p>
      </div>
    {/if}
  </div>
</div>
