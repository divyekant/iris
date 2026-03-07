<script lang="ts">
  import { api } from '../lib/api';
  import { push } from 'svelte-spa-router';
  import { ArrowLeft } from 'lucide-svelte';
  import MessageList from '../components/inbox/MessageList.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  import SkeletonRow from '../components/SkeletonRow.svelte';

  let { folder, title }: { folder: string; title: string } = $props();

  let messages = $state<any[]>([]);
  let total = $state(0);
  let loading = $state(true);
  let error = $state('');
  let selectedIds = $state(new Set<string>());

  async function loadMessages() {
    loading = true;
    error = '';
    try {
      const res = await api.messages.list({ folder });
      messages = res.messages;
      total = res.total;
    } catch (err: any) {
      error = err.message || `Failed to load ${title}`;
    } finally {
      loading = false;
    }
  }

  function handleMessageClick(id: string) {
    const msg = messages.find((m) => m.id === id);
    const threadId = msg?.thread_id || id;
    push(`/thread/${encodeURIComponent(threadId)}`);
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
  });
</script>

<div class="h-full flex flex-col">
  <!-- Folder header bar -->
  <div class="px-4 py-3 border-b flex items-center gap-3" style="border-color: var(--iris-color-border-subtle);">
    <button
      class="flex items-center justify-center w-8 h-8 rounded-lg transition-colors"
      style="color: var(--iris-color-text-muted);"
      onclick={() => push('/')}
      title="Back to Inbox"
    >
      <ArrowLeft size={18} />
    </button>
    <h2 class="text-lg font-semibold" style="color: var(--iris-color-text);">{title}</h2>
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
      <EmptyState title="No messages" subtitle="This folder is empty." />
    {:else}
      <MessageList {messages} onclick={handleMessageClick} bind:selectedIds />
    {/if}
  </div>
</div>
