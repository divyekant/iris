<script lang="ts">
  import { api } from '../lib/api';
  import { wsClient } from '../lib/ws';
  import { push } from 'svelte-spa-router';
  import MessageCard from '../components/thread/MessageCard.svelte';

  let { params }: { params: { id: string } } = $props();

  let thread = $state<any>(null);
  let loading = $state(true);
  let error = $state('');

  async function loadThread() {
    loading = true;
    error = '';
    try {
      thread = await api.threads.get(params.id);
      for (const msg of thread.messages) {
        if (!msg.is_read) {
          await api.messages.markRead(msg.id);
          msg.is_read = true;
        }
      }
    } catch (e: any) {
      error = e.message || 'Failed to load thread';
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (params.id) {
      loadThread();
    }

    const off = wsClient.on('NewEmail', () => {
      if (thread) loadThread();
    });

    return () => off();
  });
</script>

<div class="h-full flex flex-col">
  <div class="px-4 py-3 border-b border-gray-200 dark:border-gray-700 flex items-center gap-3">
    <button
      class="p-1 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 transition-colors"
      onclick={() => push('/')}
      title="Back to inbox"
    >
      &larr;
    </button>
    {#if thread}
      <div class="flex-1 min-w-0">
        <h2 class="text-lg font-semibold truncate">{thread.subject || '(no subject)'}</h2>
        <p class="text-xs text-gray-500 truncate">
          {thread.participants.map((p: any) => p.name || p.email).join(', ')}
          &middot; {thread.message_count} message{thread.message_count === 1 ? '' : 's'}
        </p>
      </div>
    {/if}
  </div>

  <div class="flex-1 overflow-y-auto p-4 space-y-3">
    {#if loading}
      <div class="flex items-center justify-center py-16">
        <div class="w-8 h-8 border-4 border-blue-200 border-t-blue-600 rounded-full animate-spin"></div>
      </div>
    {:else if error}
      <div class="text-center py-16">
        <p class="text-red-500 dark:text-red-400 mb-4">{error}</p>
        <button
          class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg text-sm font-medium"
          onclick={loadThread}
        >
          Retry
        </button>
      </div>
    {:else if thread}
      {#each thread.messages as message (message.id)}
        <MessageCard {message} />
      {/each}
    {/if}
  </div>

  {#if thread && !loading}
    <div class="px-4 py-3 border-t border-gray-200 dark:border-gray-700 flex gap-2">
      <button disabled class="px-4 py-2 text-sm bg-gray-100 dark:bg-gray-800 text-gray-400 rounded-lg cursor-not-allowed" title="Coming in V3">Reply</button>
      <button disabled class="px-4 py-2 text-sm bg-gray-100 dark:bg-gray-800 text-gray-400 rounded-lg cursor-not-allowed" title="Coming in V3">Reply All</button>
      <button disabled class="px-4 py-2 text-sm bg-gray-100 dark:bg-gray-800 text-gray-400 rounded-lg cursor-not-allowed" title="Coming in V3">Forward</button>
    </div>
  {/if}
</div>
