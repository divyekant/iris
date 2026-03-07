<script lang="ts">
  import { api } from '../lib/api';
  import { push } from 'svelte-spa-router';
  import { wsClient } from '../lib/ws';
  import MessageList from '../components/inbox/MessageList.svelte';
  import SyncStatus from '../components/inbox/SyncStatus.svelte';
  import ComposeModal from '../components/compose/ComposeModal.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  import SkeletonRow from '../components/SkeletonRow.svelte';
  import Toast from '../components/Toast.svelte';
  import { RefreshCw } from 'lucide-svelte';

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
  let page = $state(1);
  const PAGE_SIZE = 25;
  let refreshing = $state(false);
  let toastMessage = $state('');
  let toastVisible = $state(false);

  async function handleRefresh() {
    refreshing = true;
    await loadMessages();
    refreshing = false;
  }

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
        limit: PAGE_SIZE,
        offset: (page - 1) * PAGE_SIZE,
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

  function onAccountSwitch(e: Event) {
    const detail = (e as CustomEvent).detail;
    filterAccountId = detail.accountId || '';
    page = 1;
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

  async function handleRowAction(id: string, action: string) {
    try {
      await api.messages.batch([id], action);
      await loadMessages();
    } catch (e: any) {
      error = e.message || 'Action failed';
    }
  }

  $effect(() => {
    loadMessages();
    wsClient.connect();
    const offNewEmail = wsClient.on('NewEmail', (evt: any) => {
      toastMessage = `New email received`;
      toastVisible = true;
      loadMessages();
    });
    const pollInterval = setInterval(() => { loadMessages(); }, 60000);
    window.addEventListener('account-switch', onAccountSwitch);
    window.addEventListener('open-compose', onOpenCompose);
    return () => {
      clearInterval(pollInterval);
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
    {#if total > 0}
      <span class="text-xs" style="color: var(--iris-color-text-faint);">
        {total} message{total === 1 ? '' : 's'}
      </span>
    {/if}
    <button
      class="p-1.5 rounded-lg transition-colors"
      style="color: var(--iris-color-text-faint);"
      onclick={handleRefresh}
      title="Refresh"
      disabled={refreshing}
    >
      <RefreshCw size={14} class={refreshing ? 'animate-spin' : ''} />
    </button>
  </div>

  <!-- Category tabs -->
  <div class="px-4 border-b flex gap-1 overflow-x-auto" style="border-color: var(--iris-color-border-subtle);">
    {#each categories as cat}
      <button
        class="px-3 py-2 text-sm whitespace-nowrap border-b-2 transition-colors {activeCategory === cat.id ? 'font-medium' : ''}"
        style={activeCategory === cat.id
          ? 'border-color: var(--iris-color-primary); color: var(--iris-color-primary);'
          : 'border-color: transparent; color: var(--iris-color-text-muted);'}
        onclick={() => { activeCategory = cat.id; page = 1; loadMessages(); }}
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
      <MessageList {messages} onclick={handleMessageClick} bind:selectedIds onaction={handleRowAction} />
    {/if}
  </div>

  {#if !loading && total > PAGE_SIZE}
    <div class="flex items-center justify-center gap-4 py-3 border-t" style="border-color: var(--iris-color-border-subtle);">
      <button
        class="px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-30"
        style="background: var(--iris-color-bg-surface); color: var(--iris-color-text-muted); border: 1px solid var(--iris-color-border);"
        disabled={page <= 1}
        onclick={() => { page--; loadMessages(); }}
      >Previous</button>
      <span class="text-xs" style="color: var(--iris-color-text-faint);">
        {(page - 1) * PAGE_SIZE + 1}&ndash;{Math.min(page * PAGE_SIZE, total)} of {total}
      </span>
      <button
        class="px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-30"
        style="background: var(--iris-color-bg-surface); color: var(--iris-color-text-muted); border: 1px solid var(--iris-color-border);"
        disabled={page * PAGE_SIZE >= total}
        onclick={() => { page++; loadMessages(); }}
      >Next</button>
    </div>
  {/if}
</div>

<Toast message={toastMessage} visible={toastVisible} ondismiss={() => toastVisible = false} />

{#if showCompose}
  <ComposeModal
    context={{ mode: 'new', accountId: activeAccountId }}
    onclose={() => (showCompose = false)}
    onsent={loadMessages}
  />
{/if}
