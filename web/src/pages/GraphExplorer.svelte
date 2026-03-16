<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../lib/api';
  import { push } from 'svelte-spa-router';
  import { irisFade } from '../lib/transitions';
  import { ArrowLeft, Search, Users, Building2, FolderKanban, Calendar, DollarSign, Link2, ChevronRight } from 'lucide-svelte';
  import EmptyState from '../components/EmptyState.svelte';

  let searchQuery = $state('');
  let results = $state<any[]>([]);
  let loading = $state(false);
  let searched = $state(false);

  // Entity type filter
  let typeFilter = $state<string | null>(null);
  let allEntities = $state<any[]>([]);
  let totalEntities = $state(0);
  let loadingEntities = $state(false);

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  const typeIcons: Record<string, any> = {
    person: Users,
    org: Building2,
    project: FolderKanban,
    date: Calendar,
    amount: DollarSign,
  };

  const typeColors: Record<string, string> = {
    person: 'var(--iris-color-info)',
    org: 'var(--iris-color-success)',
    project: 'var(--iris-color-warning)',
    date: 'var(--iris-color-error)',
    amount: 'var(--iris-color-primary)',
  };

  async function doSearch() {
    if (!searchQuery.trim()) {
      results = [];
      searched = false;
      return;
    }
    loading = true;
    searched = true;
    try {
      const res = await api.graph.query(searchQuery);
      results = res.results;
    } catch {
      results = [];
    } finally {
      loading = false;
    }
  }

  function onInput() {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(doSearch, 300);
  }

  async function loadEntities(type?: string | null) {
    loadingEntities = true;
    try {
      const res = await api.graph.entities(type || undefined, 50);
      allEntities = res.entities;
      totalEntities = res.total;
    } catch {
      allEntities = [];
      totalEntities = 0;
    } finally {
      loadingEntities = false;
    }
  }

  function selectType(type: string | null) {
    typeFilter = type;
    loadEntities(type);
  }

  function navigateToEntity(name: string) {
    searchQuery = name;
    doSearch();
  }

  function navigateToThread(threadId: string) {
    push(`/thread/${encodeURIComponent(threadId)}`);
  }

  function formatDate(timestamp: number): string {
    const d = new Date(timestamp * 1000);
    return d.toLocaleDateString([], { month: 'short', day: 'numeric', year: 'numeric' });
  }

  onMount(() => {
    loadEntities();
  });
</script>

<div class="h-full flex flex-col">
  <!-- Header -->
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
          placeholder="Search entities... (people, orgs, projects)"
          class="w-full px-3 py-2 rounded-lg text-sm focus:outline-none focus:ring-2"
          style="background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
          autofocus
        />
      </div>
    </div>

    <!-- Type filter chips -->
    <div class="flex gap-2 mt-2">
      <button
        class="px-3 py-1 text-xs rounded-full border transition-colors"
        style={typeFilter === null
          ? 'background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent); color: var(--iris-color-primary); border-color: var(--iris-color-primary);'
          : 'background: transparent; color: var(--iris-color-text-muted); border-color: var(--iris-color-border);'}
        onclick={() => selectType(null)}
      >All</button>
      {#each ['person', 'org', 'project', 'date', 'amount'] as type}
        <button
          class="px-3 py-1 text-xs rounded-full border transition-colors capitalize"
          style={typeFilter === type
            ? `background: color-mix(in srgb, ${typeColors[type]} 10%, transparent); color: ${typeColors[type]}; border-color: ${typeColors[type]};`
            : 'background: transparent; color: var(--iris-color-text-muted); border-color: var(--iris-color-border);'}
          onclick={() => selectType(type)}
        >{type}</button>
      {/each}
    </div>
  </div>

  <!-- Content -->
  <div class="flex-1 overflow-auto px-4 py-3">
    {#if loading}
      <div class="flex items-center justify-center py-16">
        <div class="w-8 h-8 border-4 rounded-full animate-spin" style="border-color: color-mix(in srgb, var(--iris-color-primary) 20%, transparent); border-top-color: var(--iris-color-primary);"></div>
      </div>
    {:else if searched && results.length === 0}
      <EmptyState icon="search" title="No entities found" subtitle="Try a different search term." />
    {:else if searched && results.length > 0}
      <!-- Search Results -->
      <div class="text-xs mb-3" style="color: var(--iris-color-text-faint);">
        {results.length} result{results.length === 1 ? '' : 's'} for "{searchQuery}"
      </div>
      <div class="space-y-3">
        {#each results as item (item.entity.id)}
          <div
            class="rounded-lg border p-4"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface);"
            transition:irisFade
          >
            <!-- Entity header -->
            <div class="flex items-center gap-2 mb-2">
              <div
                class="w-8 h-8 rounded-full flex items-center justify-center flex-shrink-0"
                style="background: color-mix(in srgb, {typeColors[item.entity.entity_type] || 'var(--iris-color-text-muted)'} 15%, transparent);"
              >
                {#if typeIcons[item.entity.entity_type]}
                  <svelte:component this={typeIcons[item.entity.entity_type]} size={14} color={typeColors[item.entity.entity_type]} />
                {:else}
                  <Search size={14} color="var(--iris-color-text-muted)" />
                {/if}
              </div>
              <div class="flex-1 min-w-0">
                <div class="text-sm font-medium" style="color: var(--iris-color-text);">
                  {item.entity.canonical_name}
                </div>
                <span
                  class="text-xs px-1.5 py-0.5 rounded capitalize"
                  style="background: color-mix(in srgb, {typeColors[item.entity.entity_type] || 'var(--iris-color-text-muted)'} 10%, transparent); color: {typeColors[item.entity.entity_type] || 'var(--iris-color-text-muted)'};"
                >{item.entity.entity_type}</span>
              </div>
            </div>

            <!-- Aliases -->
            {#if item.aliases && item.aliases.length > 0}
              <div class="mb-2">
                <span class="text-xs font-medium" style="color: var(--iris-color-text-muted);">Also known as:</span>
                <div class="flex flex-wrap gap-1 mt-1">
                  {#each item.aliases as alias}
                    <span
                      class="text-xs px-2 py-0.5 rounded-full"
                      style="background: color-mix(in srgb, var(--iris-color-text-muted) 10%, transparent); color: var(--iris-color-text-muted);"
                    >{alias}</span>
                  {/each}
                </div>
              </div>
            {/if}

            <!-- Relations -->
            {#if item.relations && item.relations.length > 0}
              <div class="mb-2">
                <span class="text-xs font-medium" style="color: var(--iris-color-text-muted);">Relationships:</span>
                <div class="mt-1 space-y-1">
                  {#each item.relations as rel}
                    <button
                      class="flex items-center gap-1.5 text-xs transition-colors group"
                      style="color: var(--iris-color-text);"
                      onclick={() => navigateToEntity(rel.canonical_name)}
                    >
                      <Link2 size={10} style="color: var(--iris-color-text-faint);" />
                      <span class="font-mono text-xs" style="color: var(--iris-color-text-faint);">{rel.relation_type.replace(/_/g, ' ')}</span>
                      <ChevronRight size={10} style="color: var(--iris-color-text-faint);" />
                      <span class="group-hover:underline" style="color: {typeColors[rel.entity_type] || 'var(--iris-color-text)'};">{rel.canonical_name}</span>
                    </button>
                  {/each}
                </div>
              </div>
            {/if}

            <!-- Connected threads -->
            {#if item.connected_threads && item.connected_threads.length > 0}
              <div>
                <span class="text-xs font-medium" style="color: var(--iris-color-text-muted);">Mentioned in:</span>
                <div class="mt-1 space-y-0.5">
                  {#each item.connected_threads as thread}
                    <button
                      class="flex items-center gap-1.5 text-xs w-full text-left transition-colors rounded px-1.5 py-1 thread-link"
                      onclick={() => navigateToThread(thread.thread_id)}
                    >
                      <span class="truncate flex-1" style="color: var(--iris-color-text);">{thread.subject}</span>
                      {#if thread.date}
                        <span class="flex-shrink-0" style="color: var(--iris-color-text-faint);">{formatDate(thread.date)}</span>
                      {/if}
                    </button>
                  {/each}
                </div>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {:else}
      <!-- Default: Entity list -->
      {#if loadingEntities}
        <div class="flex items-center justify-center py-16">
          <div class="w-8 h-8 border-4 rounded-full animate-spin" style="border-color: color-mix(in srgb, var(--iris-color-primary) 20%, transparent); border-top-color: var(--iris-color-primary);"></div>
        </div>
      {:else if allEntities.length === 0}
        <EmptyState
          icon="network"
          title="No entities yet"
          subtitle="Entities are extracted from emails as they are processed. Enable AI and sync your inbox to start building your knowledge graph."
        />
      {:else}
        <div class="text-xs mb-3" style="color: var(--iris-color-text-faint);">
          {totalEntities} entit{totalEntities === 1 ? 'y' : 'ies'}{typeFilter ? ` of type "${typeFilter}"` : ''}
        </div>
        <div class="grid gap-2">
          {#each allEntities as entity (entity.id)}
            <button
              class="flex items-center gap-3 p-3 rounded-lg border text-left transition-colors entity-row"
              style="border-color: var(--iris-color-border-subtle);"
              onclick={() => navigateToEntity(entity.canonical_name)}
              transition:irisFade
            >
              <div
                class="w-8 h-8 rounded-full flex items-center justify-center flex-shrink-0"
                style="background: color-mix(in srgb, {typeColors[entity.entity_type] || 'var(--iris-color-text-muted)'} 15%, transparent);"
              >
                {#if typeIcons[entity.entity_type]}
                  <svelte:component this={typeIcons[entity.entity_type]} size={14} color={typeColors[entity.entity_type]} />
                {:else}
                  <Search size={14} color="var(--iris-color-text-muted)" />
                {/if}
              </div>
              <div class="flex-1 min-w-0">
                <div class="text-sm font-medium truncate" style="color: var(--iris-color-text);">{entity.canonical_name}</div>
                <span
                  class="text-xs capitalize"
                  style="color: {typeColors[entity.entity_type] || 'var(--iris-color-text-muted)'};"
                >{entity.entity_type}</span>
              </div>
              <ChevronRight size={14} style="color: var(--iris-color-text-faint);" />
            </button>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .entity-row:hover {
    background: var(--iris-color-bg-surface);
  }
  .thread-link:hover {
    background: color-mix(in srgb, var(--iris-color-primary) 5%, transparent);
  }
</style>
