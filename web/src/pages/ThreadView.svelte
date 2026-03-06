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
  <div class="px-4 py-3 flex items-center gap-3" style="border-bottom: 1px solid var(--iris-color-border);">
    <button
      class="p-1 transition-colors"
      style="color: var(--iris-color-text-muted);"
      onclick={() => push('/')}
      title="Back to inbox"
    >
      &larr;
    </button>
    {#if thread}
      <div class="flex-1 min-w-0">
        <h2 class="text-lg font-semibold truncate" style="color: var(--iris-color-text);">{thread.subject || '(no subject)'}</h2>
        <p class="text-xs truncate" style="color: var(--iris-color-text-faint);">
          {thread.participants.map((p: any) => p.name || p.email).join(', ')}
          &middot; {thread.message_count} message{thread.message_count === 1 ? '' : 's'}
        </p>
      </div>
      <div class="flex items-center gap-1">
        <button
          class="p-2 transition-colors thread-action-btn star-btn"
          onclick={() => handleThreadAction('star')}
          title="Star"
        >&#9734;</button>
        <button
          class="p-2 transition-colors thread-action-btn"
          onclick={() => handleThreadAction('archive')}
          title="Archive"
        >&#128230;</button>
        <button
          class="p-2 transition-colors thread-action-btn"
          onclick={() => handleThreadAction('mark_unread')}
          title="Mark unread"
        >&#9993;</button>
        <button
          class="p-2 transition-colors thread-action-btn delete-btn"
          onclick={() => handleThreadAction('delete')}
          title="Delete"
        >&#128465;</button>
      </div>
    {/if}
  </div>

  <!-- AI Summary panel -->
  {#if thread && thread.message_count > 1}
    <div class="px-4 py-2" style="border-bottom: 1px solid var(--iris-color-border);">
      <button
        class="text-xs flex items-center gap-1"
        style="color: var(--iris-color-primary);"
        onclick={toggleSummary}
      >
        <span class="text-[10px]">{summaryOpen ? '\u25BE' : '\u25B8'}</span> &#10024; AI Summary
      </button>
      {#if summaryOpen}
        {#if summaryLoading}
          <div class="mt-2 text-xs flex items-center gap-2" style="color: var(--iris-color-text-faint);">
            <div class="w-3 h-3 rounded-full animate-spin" style="border: 2px solid var(--iris-color-border); border-top-color: var(--iris-color-primary);"></div>
            Summarizing thread...
          </div>
        {:else if aiSummary}
          <div class="mt-2 text-sm rounded-lg px-3 py-2 leading-relaxed" style="color: var(--iris-color-text); background: var(--iris-color-bg-surface);">
            {aiSummary}
          </div>
        {:else if summaryError}
          <div class="mt-2 text-xs" style="color: var(--iris-color-text-faint);">{summaryError}</div>
        {/if}
      {/if}
    </div>
  {/if}

  <div class="flex-1 overflow-y-auto p-4 space-y-3">
    {#if loading}
      <div class="flex items-center justify-center py-16">
        <div class="w-8 h-8 rounded-full animate-spin" style="border: 4px solid var(--iris-color-border); border-top-color: var(--iris-color-primary);"></div>
      </div>
    {:else if error}
      <div class="text-center py-16">
        <p class="mb-4" style="color: var(--iris-color-error);">{error}</p>
        <button
          class="px-4 py-2 rounded-lg text-sm font-medium transition-colors retry-btn"
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
    <div class="px-4 py-3 flex gap-2" style="border-top: 1px solid var(--iris-color-border); background: var(--iris-color-bg-elevated);">
      <button
        class="px-4 py-2 text-sm rounded-lg font-medium transition-colors reply-primary-btn"
        onclick={openReply}
      >
        Reply
      </button>
      <button
        class="px-4 py-2 text-sm rounded-lg font-medium transition-colors reply-secondary-btn"
        onclick={openReplyAll}
      >
        Reply All
      </button>
      <button
        class="px-4 py-2 text-sm rounded-lg font-medium transition-colors reply-secondary-btn"
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

<style>
  .thread-action-btn {
    color: var(--iris-color-text-faint);
  }
  .thread-action-btn:hover {
    color: var(--iris-color-text-muted);
  }
  .thread-action-btn.star-btn:hover {
    color: var(--iris-color-primary);
  }
  .thread-action-btn.delete-btn:hover {
    color: var(--iris-color-error);
  }
  .retry-btn {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .retry-btn:hover {
    filter: brightness(1.1);
  }
  .reply-primary-btn {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .reply-primary-btn:hover {
    filter: brightness(1.1);
  }
  .reply-secondary-btn {
    background: var(--iris-color-bg-surface);
    color: var(--iris-color-text-muted);
    border: 1px solid var(--iris-color-border);
  }
  .reply-secondary-btn:hover {
    background: var(--iris-color-bg-elevated);
    color: var(--iris-color-text);
  }
</style>
