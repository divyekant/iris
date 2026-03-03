<script lang="ts">
  import { api } from '../lib/api';
  import { push } from 'svelte-spa-router';
  import { wsClient } from '../lib/ws';
  import MessageList from '../components/inbox/MessageList.svelte';
  import SyncStatus from '../components/inbox/SyncStatus.svelte';
  import ComposeModal from '../components/compose/ComposeModal.svelte';

  let messages = $state<any[]>([]);
  let unreadCount = $state(0);
  let total = $state(0);
  let loading = $state(true);
  let error = $state('');
  let showCompose = $state(false);
  let activeAccountId = $state('');
  let activeCategory = $state('');

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
      const res = await api.messages.list({ category: activeCategory || undefined });
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

  async function ensureAccountId() {
    if (activeAccountId) return;
    try {
      const accounts = await api.accounts.list();
      if (accounts.length > 0) activeAccountId = accounts[0].id;
    } catch { /* ignore */ }
  }

  function handleMessageClick(id: string) {
    const msg = messages.find((m) => m.id === id);
    const threadId = msg?.thread_id || id;
    push(`/thread/${encodeURIComponent(threadId)}`);
  }

  async function openCompose() {
    await ensureAccountId();
    if (!activeAccountId) {
      push('/setup');
      return;
    }
    showCompose = true;
  }

  $effect(() => {
    loadMessages();
    wsClient.connect();
    const offNewEmail = wsClient.on('NewEmail', () => { loadMessages(); });
    return () => { offNewEmail(); };
  });
</script>

<div class="h-full flex flex-col">
  <!-- Inbox header bar -->
  <div class="px-4 py-3 border-b border-gray-200 dark:border-gray-700 flex items-center gap-3">
    <h2 class="text-lg font-semibold">Inbox</h2>
    {#if unreadCount > 0}
      <span class="px-2 py-0.5 bg-blue-600 text-white text-xs font-medium rounded-full">
        {unreadCount}
      </span>
    {/if}
    <span class="flex-1"></span>
    <button
      class="px-4 py-1.5 bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium rounded-lg transition-colors"
      onclick={openCompose}
    >
      Compose
    </button>
    {#if total > 0}
      <span class="text-xs text-gray-400 dark:text-gray-500">
        {total} message{total === 1 ? '' : 's'}
      </span>
    {/if}
  </div>

  <!-- Category tabs -->
  <div class="px-4 border-b border-gray-200 dark:border-gray-700 flex gap-1 overflow-x-auto">
    {#each categories as cat}
      <button
        class="px-3 py-2 text-sm whitespace-nowrap border-b-2 transition-colors
               {activeCategory === cat.id
                 ? 'border-blue-600 text-blue-600 font-medium'
                 : 'border-transparent text-gray-500 hover:text-gray-700 dark:hover:text-gray-300'}"
        onclick={() => { activeCategory = cat.id; loadMessages(); }}
      >
        {cat.label}
      </button>
    {/each}
  </div>

  <SyncStatus />

  <div class="flex-1 overflow-auto">
    {#if loading}
      <div class="flex items-center justify-center py-16">
        <div class="w-8 h-8 border-4 border-blue-200 border-t-blue-600 rounded-full animate-spin"></div>
      </div>
    {:else if error}
      <div class="text-center py-16">
        <p class="text-red-500 dark:text-red-400 mb-4">{error}</p>
        <button
          class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg text-sm font-medium transition-colors"
          onclick={loadMessages}
        >
          Retry
        </button>
      </div>
    {:else}
      <MessageList {messages} onclick={handleMessageClick} />
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
