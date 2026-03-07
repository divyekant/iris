<script lang="ts">
  import { api } from '../lib/api';
  import { push } from 'svelte-spa-router';
  import { ArrowLeft, Paperclip } from 'lucide-svelte';
  import EmptyState from '../components/EmptyState.svelte';

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
  <div class="px-4 py-3 border-b" style="border-color: var(--iris-color-border);">
    <div class="flex items-center gap-3">
      <button
        class="p-1 transition-colors"
        style="color: var(--iris-color-text-faint);"
        onclick={() => push('/')}
        title="Back to inbox"
      ><ArrowLeft size={16} /></button>
      <input
        type="text"
        bind:value={searchQuery}
        oninput={onInput}
        placeholder="Search emails..."
        class="flex-1 px-3 py-2 rounded-lg text-sm focus:outline-none focus:ring-2"
        style="background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
        autofocus
      />
    </div>

    <!-- Filter chips -->
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
        style={hasAttachment
          ? 'background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent); color: var(--iris-color-primary); border-color: var(--iris-color-primary);'
          : 'background: transparent; color: var(--iris-color-text-muted); border-color: var(--iris-color-border);'}
        onclick={toggleAttachment}
      >has:attachment</button>
      {#each [{ id: '7d', label: 'Last 7 days' }, { id: '30d', label: 'Last 30 days' }, { id: '1y', label: 'Last year' }] as f}
        <button
          class="px-3 py-1 text-xs rounded-full border transition-colors"
          style={dateFilter === f.id
            ? 'background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent); color: var(--iris-color-primary); border-color: var(--iris-color-primary);'
            : 'background: transparent; color: var(--iris-color-text-muted); border-color: var(--iris-color-border);'}
          onclick={() => toggleFilter(f.id)}
        >{f.label}</button>
      {/each}
    </div>
  </div>

  <!-- Results -->
  <div class="flex-1 overflow-auto">
    {#if loading}
      <div class="flex items-center justify-center py-16">
        <div class="w-8 h-8 border-4 rounded-full animate-spin" style="border-color: color-mix(in srgb, var(--iris-color-primary) 20%, transparent); border-top-color: var(--iris-color-primary);"></div>
      </div>
    {:else if searched && results.length === 0}
      <EmptyState icon="search" title="No results found" subtitle="Try different keywords or remove some filters." />
    {:else if results.length > 0}
      <div class="px-4 py-2 text-xs" style="color: var(--iris-color-text-faint);">
        {total} result{total === 1 ? '' : 's'}
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
                <span class="flex-shrink-0 text-xs ml-auto" style="color: var(--iris-color-text-faint);">
                  {#if result.has_attachments}
                    <span class="mr-1.5" title="Has attachments"><Paperclip size={12} /></span>
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
        <p class="text-sm" style="color: var(--iris-color-text-muted);">Type a keyword to find messages by content, subject, or sender.</p>
      </div>
    {/if}
  </div>
</div>

<style>
  .search-result-row:hover {
    background: var(--iris-color-bg-surface);
  }
</style>
