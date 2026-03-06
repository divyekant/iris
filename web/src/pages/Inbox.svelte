<script lang="ts">
  import { api } from '../lib/api';
  import { push } from 'svelte-spa-router';
  import { wsClient } from '../lib/ws';
  import MessageList from '../components/inbox/MessageList.svelte';
  import SyncStatus from '../components/inbox/SyncStatus.svelte';
  import ComposeModal from '../components/compose/ComposeModal.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  import SkeletonRow from '../components/SkeletonRow.svelte';

  let messages = $state<any[]>([]);
  let unreadCount = $state(0);
  let total = $state(0);
  let loading = $state(true);
  let error = $state('');
  let showCompose = $state(false);
  let activeAccountId = $state('');
  let activeCategory = $state('');
  let selectedIds = $state(new Set<string>());
  let filterAccountId = $state('');
  let viewMode = $state('traditional');

  const categories = [
    { id: '', label: 'All' },
    { id: 'primary', label: 'Primary' },
    { id: 'updates', label: 'Updates' },
    { id: 'social', label: 'Social' },
    { id: 'promotions', label: 'Promotions' },
  ];

  async function loadMessages() {
    loading = true;
    error = '';
    try {
      const res = await api.messages.list({
        account_id: filterAccountId || undefined,
        category: activeCategory || undefined,
      });
      messages = res.messages;
      unreadCount = res.unread_count;
      total = res.total;
      // Track first account for compose
      if (messages.length > 0 && !activeAccountId) {
        activeAccountId = messages[0].account_id;
      }
    } catch (err: any) {
      error = err.message || 'Failed to load messages';
    } finally {
      loading = false;
    }
  }

  function handleMessageClick(id: string) {
    const msg = messages.find((m) => m.id === id);
    const threadId = msg?.thread_id || id;
    push(`/thread/${encodeURIComponent(threadId)}`);
  }

  async function loadViewMode() {
    try {
      const config = await api.config.get();
      viewMode = config.view_mode || 'traditional';
    } catch { /* ignore */ }
  }

  async function toggleViewMode() {
    const next = viewMode === 'traditional' ? 'messaging' : 'traditional';
    viewMode = next;
    try {
      await api.config.setViewMode(next);
    } catch { /* ignore */ }
  }

  function onAccountSwitch(e: Event) {
    const detail = (e as CustomEvent).detail;
    filterAccountId = detail.accountId || '';
    loadMessages();
  }

  function onOpenCompose(e: Event) {
    const detail = (e as CustomEvent).detail;
    if (detail.accountId) activeAccountId = detail.accountId;
    showCompose = true;
  }

  async function handleBulkAction(action: string) {
    if (selectedIds.size === 0) return;
    try {
      await api.messages.batch([...selectedIds], action);
      selectedIds = new Set();
      await loadMessages();
    } catch (e: any) {
      error = e.message || 'Bulk action failed';
    }
  }

  $effect(() => {
    loadMessages();
    loadViewMode();
    wsClient.connect();
    const offNewEmail = wsClient.on('NewEmail', () => { loadMessages(); });
    window.addEventListener('account-switch', onAccountSwitch);
    window.addEventListener('open-compose', onOpenCompose);
    return () => {
      offNewEmail();
      window.removeEventListener('account-switch', onAccountSwitch);
      window.removeEventListener('open-compose', onOpenCompose);
    };
  });
</script>

<div class="h-full flex flex-col">
  <!-- Inbox header bar -->
  <div class="px-4 py-3 border-b flex items-center gap-3" style="border-color: var(--iris-color-border-subtle);">
    <h2 class="text-lg font-semibold">Inbox</h2>
    {#if unreadCount > 0}
      <span class="px-2 py-0.5 text-xs font-medium rounded-full" style="background: var(--iris-color-primary); color: var(--iris-color-bg);">
        {unreadCount}
      </span>
    {/if}
    <span class="flex-1"></span>
    <button
      class="px-3 py-1.5 text-xs font-medium rounded-lg border transition-colors"
      style={viewMode === 'traditional'
        ? 'border-color: var(--iris-color-border); color: var(--iris-color-text-muted);'
        : 'border-color: var(--iris-color-primary); color: var(--iris-color-primary); background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent);'}
      onclick={toggleViewMode}
      title="Toggle view mode"
    >
      {viewMode === 'traditional' ? 'Traditional' : 'Messaging'}
    </button>
    {#if total > 0}
      <span class="text-xs" style="color: var(--iris-color-text-faint);">
        {total} message{total === 1 ? '' : 's'}
      </span>
    {/if}
  </div>

  <!-- Category tabs -->
  <div class="px-4 border-b flex gap-1 overflow-x-auto" style="border-color: var(--iris-color-border-subtle);">
    {#each categories as cat}
      <button
        class="px-3 py-2 text-sm whitespace-nowrap border-b-2 transition-colors {activeCategory === cat.id ? 'font-medium' : ''}"
        style={activeCategory === cat.id
          ? 'border-color: var(--iris-color-primary); color: var(--iris-color-primary);'
          : 'border-color: transparent; color: var(--iris-color-text-muted);'}
        onclick={() => { activeCategory = cat.id; loadMessages(); }}
      >
        {cat.label}
      </button>
    {/each}
  </div>

  <SyncStatus />

  {#if selectedIds.size > 0}
    <div class="px-4 py-2 border-b flex items-center gap-2" style="background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent); border-color: var(--iris-color-border);">
      <span class="text-sm font-medium" style="color: var(--iris-color-primary);">
        {selectedIds.size} selected
      </span>
      <span class="flex-1"></span>
      <button class="px-3 py-1 text-xs font-medium rounded transition-colors" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border); color: var(--iris-color-text-muted);" onclick={() => handleBulkAction('archive')}>Archive</button>
      <button class="px-3 py-1 text-xs font-medium rounded transition-colors" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border); color: var(--iris-color-text-muted);" onclick={() => handleBulkAction('mark_read')}>Mark Read</button>
      <button class="px-3 py-1 text-xs font-medium rounded transition-colors" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border); color: var(--iris-color-text-muted);" onclick={() => handleBulkAction('mark_unread')}>Mark Unread</button>
      <button class="px-3 py-1 text-xs font-medium rounded transition-colors" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border); color: var(--iris-color-text-muted);" onclick={() => handleBulkAction('star')}>Star</button>
      <button class="px-3 py-1 text-xs font-medium rounded transition-colors" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border); color: var(--iris-color-error);" onclick={() => handleBulkAction('delete')}>Delete</button>
      <button class="px-3 py-1 text-xs" style="color: var(--iris-color-text-faint);" onclick={() => (selectedIds = new Set())}>Clear</button>
    </div>
  {/if}

  <div class="flex-1 overflow-auto">
    {#if loading}
      <SkeletonRow widths={[120, 280, 200]} />
      <SkeletonRow widths={[160, 340, 180]} />
      <SkeletonRow widths={[100, 300, 220]} />
      <SkeletonRow widths={[140, 260, 240]} />
    {:else if error}
      <div class="text-center py-16">
        <p class="mb-4" style="color: var(--iris-color-error);">{error}</p>
        <button
          class="px-4 py-2 rounded-lg text-sm font-medium transition-colors"
          style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
          onclick={loadMessages}
        >
          Retry
        </button>
      </div>
    {:else if messages.length === 0}
      <EmptyState title="Inbox zero" subtitle="You've read everything. Time to go outside." />
    {:else}
      <MessageList {messages} onclick={handleMessageClick} bind:selectedIds />
    {/if}
  </div>
</div>

{#if showCompose}
  <ComposeModal
    context={{ mode: 'new', accountId: activeAccountId }}
    onclose={() => (showCompose = false)}
    onsent={loadMessages}
  />
{/if}
