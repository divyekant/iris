<script lang="ts">
  import { api } from '../lib/api';
  import { push } from 'svelte-spa-router';
  import { wsClient } from '../lib/ws';
  import MessageList from '../components/inbox/MessageList.svelte';
  import SyncStatus from '../components/inbox/SyncStatus.svelte';

  let messages = $state<any[]>([]);
  let unreadCount = $state(0);
  let total = $state(0);
  let loading = $state(true);
  let error = $state('');

  async function loadMessages() {
    loading = true;
    error = '';
    try {
      const res = await api.messages.list();
      messages = res.messages;
      unreadCount = res.unread_count;
      total = res.total;
    } catch (err: any) {
      error = err.message || 'Failed to load messages';
    } finally {
      loading = false;
    }
  }

  function handleMessageClick(id: string) {
    const msg = messages.find((m) => m.id === id);
    const threadId = msg?.thread_id || id;
    push(`/thread/${threadId}`);
  }

  // Load messages and connect WebSocket on mount
  $effect(() => {
    loadMessages();

    wsClient.connect();

    const offNewEmail = wsClient.on('NewEmail', () => {
      loadMessages();
    });

    return () => {
      offNewEmail();
    };
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
    {#if total > 0}
      <span class="text-xs text-gray-400 dark:text-gray-500 ml-auto">
        {total} message{total === 1 ? '' : 's'}
      </span>
    {/if}
  </div>

  <!-- Sync status bar -->
  <SyncStatus />

  <!-- Message list -->
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
