<script lang="ts">
  import { api } from '../lib/api';
  import { wsClient } from '../lib/ws';
  import { push } from 'svelte-spa-router';
  import MessageCard from '../components/thread/MessageCard.svelte';
  import ComposeModal from '../components/compose/ComposeModal.svelte';

  let { params }: { params: { id: string } } = $props();

  let thread = $state<any>(null);
  let loading = $state(true);
  let error = $state('');
  let composeContext = $state<any>(null);

  // AI summary state
  let aiSummary = $state<string | null>(null);
  let summaryLoading = $state(false);
  let summaryOpen = $state(false);
  let summaryError = $state('');

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

  function lastMessage() {
    return thread?.messages?.[thread.messages.length - 1];
  }

  function openReply() {
    const msg = lastMessage();
    if (!msg) return;
    composeContext = {
      mode: 'reply',
      accountId: msg.account_id,
      original: {
        message_id: msg.message_id || msg.id,
        from_address: msg.from_address,
        from_name: msg.from_name,
        to_addresses: msg.to_addresses,
        cc_addresses: msg.cc_addresses,
        subject: thread.subject,
        body_text: msg.body_text,
        date: msg.date,
      },
    };
  }

  function openReplyAll() {
    const msg = lastMessage();
    if (!msg) return;
    composeContext = {
      mode: 'reply-all',
      accountId: msg.account_id,
      original: {
        message_id: msg.message_id || msg.id,
        from_address: msg.from_address,
        from_name: msg.from_name,
        to_addresses: msg.to_addresses,
        cc_addresses: msg.cc_addresses,
        subject: thread.subject,
        body_text: msg.body_text,
        date: msg.date,
      },
    };
  }

  function openForward() {
    const msg = lastMessage();
    if (!msg) return;
    composeContext = {
      mode: 'forward',
      accountId: msg.account_id,
      original: {
        from_address: msg.from_address,
        from_name: msg.from_name,
        subject: thread.subject,
        body_text: msg.body_text,
        date: msg.date,
      },
    };
  }

  async function handleThreadAction(action: string) {
    if (!thread) return;
    const ids = thread.messages.map((m: any) => m.id);
    try {
      await api.messages.batch(ids, action);
      if (action === 'archive' || action === 'delete') {
        push('/');
      } else {
        await loadThread();
      }
    } catch (e: any) {
      error = e.message || 'Action failed';
    }
  }

  async function toggleSummary() {
    if (summaryOpen) {
      summaryOpen = false;
      return;
    }
    summaryOpen = true;
    if (aiSummary) return; // already loaded
    summaryLoading = true;
    summaryError = '';
    try {
      const res = await api.threads.summarize(params.id);
      aiSummary = res.summary;
    } catch (e: any) {
      if (e.message?.includes('503')) {
        summaryError = 'Enable AI in Settings to use this feature.';
      } else {
        summaryError = 'Failed to generate summary.';
      }
    } finally {
      summaryLoading = false;
    }
  }

  $effect(() => {
    if (params.id) {
      // Reset AI summary when navigating to a new thread
      aiSummary = null;
      summaryOpen = false;
      summaryError = '';
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
      <div class="flex items-center gap-1">
        <button
          class="p-2 text-gray-400 hover:text-yellow-500 transition-colors"
          onclick={() => handleThreadAction('star')}
          title="Star"
        >&#9734;</button>
        <button
          class="p-2 text-gray-400 hover:text-gray-700 dark:hover:text-gray-300 transition-colors"
          onclick={() => handleThreadAction('archive')}
          title="Archive"
        >&#128230;</button>
        <button
          class="p-2 text-gray-400 hover:text-gray-700 dark:hover:text-gray-300 transition-colors"
          onclick={() => handleThreadAction('mark_unread')}
          title="Mark unread"
        >&#9993;</button>
        <button
          class="p-2 text-gray-400 hover:text-red-500 transition-colors"
          onclick={() => handleThreadAction('delete')}
          title="Delete"
        >&#128465;</button>
      </div>
    {/if}
  </div>

  <!-- AI Summary panel -->
  {#if thread && thread.message_count > 1}
    <div class="px-4 py-2 border-b border-gray-200 dark:border-gray-700">
      <button
        class="text-xs text-blue-500 hover:text-blue-600 flex items-center gap-1"
        onclick={toggleSummary}
      >
        <span class="text-[10px]">{summaryOpen ? '\u25BE' : '\u25B8'}</span> AI Summary
      </button>
      {#if summaryOpen}
        {#if summaryLoading}
          <div class="mt-2 text-xs text-gray-400 dark:text-gray-500 flex items-center gap-2">
            <div class="w-3 h-3 border-2 border-blue-200 border-t-blue-500 rounded-full animate-spin"></div>
            Summarizing thread...
          </div>
        {:else if aiSummary}
          <div class="mt-2 text-sm text-gray-700 dark:text-gray-300 bg-blue-50 dark:bg-blue-900/20 rounded-lg px-3 py-2 leading-relaxed">
            {aiSummary}
          </div>
        {:else if summaryError}
          <div class="mt-2 text-xs text-gray-400 dark:text-gray-500">{summaryError}</div>
        {/if}
      {/if}
    </div>
  {/if}

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
      <button
        class="px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 text-white rounded-lg font-medium transition-colors"
        onclick={openReply}
      >
        Reply
      </button>
      <button
        class="px-4 py-2 text-sm bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-700 dark:text-gray-300 rounded-lg font-medium transition-colors"
        onclick={openReplyAll}
      >
        Reply All
      </button>
      <button
        class="px-4 py-2 text-sm bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 text-gray-700 dark:text-gray-300 rounded-lg font-medium transition-colors"
        onclick={openForward}
      >
        Forward
      </button>
    </div>
  {/if}
</div>

{#if composeContext}
  <ComposeModal
    context={composeContext}
    onclose={() => (composeContext = null)}
    onsent={loadThread}
  />
{/if}
