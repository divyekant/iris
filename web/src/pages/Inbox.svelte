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
  import SpamDialog from '../components/SpamDialog.svelte';


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
  let toastMessage = $state('');
  let toastVisible = $state(false);
  let showSpamDialog = $state(false);
  let spamTargetId = $state('');
  let spamTargetEmail = $state('');

  function onCategoryChange(e: Event) {
    const detail = (e as CustomEvent).detail;
    activeCategory = detail.category || '';
    page = 1;
    loadMessages();
  }

  async function loadMessages(showLoading = true) {
    if (showLoading) loading = true;
    error = '';
    try {
      const res = await api.messages.list({
        account_id: filterAccountId || undefined,
        category: activeCategory || undefined,
        limit: PAGE_SIZE,
        offset: (page - 1) * PAGE_SIZE,
      });
      // Only update if data actually changed to prevent unnecessary re-renders
      if (JSON.stringify(res.messages.map((m: any) => m.id + m.is_read + m.is_starred)) !==
          JSON.stringify(messages.map((m: any) => m.id + m.is_read + m.is_starred))) {
        messages = res.messages;
      }
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
    if (action === 'report_spam') {
      const msg = messages.find(m => m.id === id);
      spamTargetId = id;
      spamTargetEmail = msg?.from_address || 'Unknown sender';
      showSpamDialog = true;
      return;
    }
    try {
      await api.messages.batch([id], action);
      await loadMessages();
    } catch (e: any) {
      error = e.message || 'Action failed';
    }
  }

  async function handleReportSpam(blockSender: boolean) {
    try {
      await api.messages.reportSpam([spamTargetId], blockSender);
      showSpamDialog = false;
      spamTargetId = '';
      spamTargetEmail = '';
      await loadMessages();
    } catch (e: any) {
      error = e.message || 'Failed to report spam';
      showSpamDialog = false;
    }
  }

  $effect(() => {
    loadMessages();
    wsClient.connect();
    const offNewEmail = wsClient.on('NewEmail', (evt: any) => {
      toastMessage = `New email received`;
      toastVisible = true;
      loadMessages(false);
    });
    const pollInterval = setInterval(() => { loadMessages(false); }, 60000);
    window.addEventListener('account-switch', onAccountSwitch);
    window.addEventListener('open-compose', onOpenCompose);
    window.addEventListener('category-change', onCategoryChange);
    return () => {
      clearInterval(pollInterval);
      offNewEmail();
      window.removeEventListener('account-switch', onAccountSwitch);
      window.removeEventListener('open-compose', onOpenCompose);
      window.removeEventListener('category-change', onCategoryChange);
    };
  });
</script>

<div class="h-full flex flex-col">
  <!-- No sub-header — tabs are in TopNav. Just a sync status bar below. -->

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

{#if showSpamDialog}
  <SpamDialog
    senderEmail={spamTargetEmail}
    messageIds={[spamTargetId]}
    onconfirm={handleReportSpam}
    onclose={() => { showSpamDialog = false; spamTargetId = ''; spamTargetEmail = ''; }}
  />
{/if}
