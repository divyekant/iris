<script lang="ts">
  import { api } from '../lib/api';
  import { push } from 'svelte-spa-router';
  import { ArrowLeft, Clock } from 'lucide-svelte';
  import MessageList from '../components/inbox/MessageList.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  import SkeletonRow from '../components/SkeletonRow.svelte';
  import { feedback } from '../lib/feedback';

  let messages = $state<any[]>([]);
  let total = $state(0);
  let loading = $state(true);
  let error = $state('');
  let selectedIds = $state(new Set<string>());

  async function loadMessages() {
    loading = true;
    error = '';
    try {
      const res = await api.messages.listSnoozed();
      messages = res.messages;
      total = res.total;
    } catch (err: any) {
      error = err.message || 'Failed to load snoozed messages';
    } finally {
      loading = false;
    }
  }

  function handleMessageClick(id: string) {
    const msg = messages.find((m) => m.id === id);
    const threadId = msg?.thread_id || id;
    push(`/thread/${encodeURIComponent(threadId)}`);
  }

  async function handleRowAction(id: string, action: string) {
    try {
      if (action === 'unsnooze') {
        await api.messages.unsnooze([id]);
        feedback.success('Email unsnoozed');
      } else {
        await api.messages.batch([id], action);
      }
      await loadMessages();
    } catch (e: any) {
      error = e.message || 'Action failed';
    }
  }

  async function handleBulkAction(action: string) {
    if (selectedIds.size === 0) return;
    try {
      if (action === 'unsnooze') {
        await api.messages.unsnooze([...selectedIds]);
        feedback.success(`${selectedIds.size} email(s) unsnoozed`);
      } else {
        await api.messages.batch([...selectedIds], action);
      }
      selectedIds = new Set();
      await loadMessages();
    } catch (e: any) {
      error = e.message || 'Bulk action failed';
    }
  }

  $effect(() => {
    loadMessages();
  });
</script>

<div class="h-full flex flex-col">
  <div class="px-4 py-3 border-b flex items-center gap-3" style="border-color: var(--iris-color-border-subtle);">
    <button
      class="flex items-center justify-center w-8 h-8 rounded-lg transition-colors"
      style="color: var(--iris-color-text-muted);"
      onclick={() => push('/')}
      title="Back to Inbox"
    >
      <ArrowLeft size={18} />
    </button>
    <Clock size={18} style="color: var(--iris-color-warning);" />
    <h2 class="text-lg font-semibold" style="color: var(--iris-color-text);">Snoozed</h2>
    <span class="flex-1"></span>
    {#if total > 0}
      <span class="text-xs" style="color: var(--iris-color-text-faint);">
        {total} message{total === 1 ? '' : 's'}
      </span>
    {/if}
  </div>

  {#if selectedIds.size > 0}
    <div class="px-4 py-2 border-b flex items-center gap-2" style="background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent); border-color: var(--iris-color-border);">
      <span class="text-sm font-medium" style="color: var(--iris-color-primary);">
        {selectedIds.size} selected
      </span>
      <span class="flex-1"></span>
      <button class="px-3 py-1 text-xs font-medium rounded transition-colors" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border); color: var(--iris-color-warning);" onclick={() => handleBulkAction('unsnooze')}>Unsnooze</button>
      <button class="px-3 py-1 text-xs font-medium rounded transition-colors" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border); color: var(--iris-color-text-muted);" onclick={() => handleBulkAction('archive')}>Archive</button>
      <button class="px-3 py-1 text-xs font-medium rounded transition-colors" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border); color: var(--iris-color-error);" onclick={() => handleBulkAction('delete')}>Delete</button>
      <button class="px-3 py-1 text-xs" style="color: var(--iris-color-text-faint);" onclick={() => (selectedIds = new Set())}>Clear</button>
    </div>
  {/if}

  <div class="flex-1 overflow-auto">
    {#if loading}
      <SkeletonRow widths={[120, 280, 200]} />
      <SkeletonRow widths={[160, 340, 180]} />
      <SkeletonRow widths={[100, 300, 220]} />
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
      <EmptyState title="No snoozed emails" subtitle="Snoozed emails will appear here until their wake-up time." />
    {:else}
      <MessageList {messages} onclick={handleMessageClick} bind:selectedIds onaction={handleRowAction} />
    {/if}
  </div>
</div>

