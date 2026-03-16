<script lang="ts">
  import { api } from '../lib/api';
  import { push } from 'svelte-spa-router';
  import { wsClient } from '../lib/ws';
  import { showNotification, init as initNotifications } from '../lib/notifications';
  import MessageList from '../components/inbox/MessageList.svelte';
  import SyncStatus from '../components/inbox/SyncStatus.svelte';
  import ComposeModal from '../components/compose/ComposeModal.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  import SkeletonRow from '../components/SkeletonRow.svelte';
  import { feedback } from '../lib/feedback';
  import SpamDialog from '../components/SpamDialog.svelte';
  import ContactTopicsPanel from '../components/contacts/ContactTopicsPanel.svelte';
  import { registerShortcuts, currentMode, getAllShortcuts } from '$lib/keyboard';


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

  let showSpamDialog = $state(false);
  let spamTargetId = $state('');
  let spamTargetEmail = $state('');
  let mutedThreadIds = $state(new Set<string>());
  let topicsEmail = $state<string | null>(null);
  let topicsName = $state<string | null>(null);
  let notificationPrefs = $state<Record<string, boolean>>({});

  // Keyboard navigation state
  let focusedIndex = $state(-1);
  let showShortcutHelp = $state(false);

  function scrollFocusedIntoView() {
    // Defer to next tick so the DOM has updated
    requestAnimationFrame(() => {
      const container = document.querySelector('.flex-1.overflow-auto');
      if (!container) return;
      const rows = container.querySelectorAll('[role="button"]');
      const row = rows[focusedIndex] as HTMLElement | undefined;
      if (row) {
        row.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
      }
    });
  }

  async function loadMutedThreads() {
    try {
      const muted = await api.mutedThreads.list();
      mutedThreadIds = new Set(muted);
    } catch {
      // Non-critical — silently ignore
    }
  }

  async function loadNotificationPrefs() {
    try {
      const accounts = await api.accounts.list();
      await Promise.all(accounts.map(async (account: any) => {
        try {
          const res = await api.notifications.get(account.id);
          notificationPrefs[account.id] = res.enabled;
        } catch {
          notificationPrefs[account.id] = true;
        }
      }));
    } catch { /* ignore */ }
  }

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
      if (activeCategory === '__needs_reply__') {
        // Fetch from dedicated needs-reply endpoint
        const res = await api.messages.needsReply({
          account_id: filterAccountId || undefined,
          limit: PAGE_SIZE,
          offset: (page - 1) * PAGE_SIZE,
        });
        if (JSON.stringify(res.messages.map((m: any) => m.id + m.is_read + m.is_starred)) !==
            JSON.stringify(messages.map((m: any) => m.id + m.is_read + m.is_starred))) {
          messages = res.messages;
        }
        unreadCount = res.total;
        total = res.total;
      } else {
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
      }
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
    const ids = [...selectedIds];
    try {
      await api.messages.batch(ids, action);
      selectedIds = new Set();
      await loadMessages();
      if (action === 'archive') {
        feedback.success(`${ids.length} archived`, { undo: async () => { await api.messages.batch(ids, 'unarchive'); await loadMessages(); } });
      } else if (action === 'delete') {
        feedback.success(`${ids.length} deleted`);
      } else if (action === 'star') {
        feedback.success(`${ids.length} starred`);
      } else if (action === 'mark_read') {
        feedback.success(`${ids.length} marked as read`);
      } else if (action === 'mark_unread') {
        feedback.success(`${ids.length} marked as unread`);
      }
    } catch (e: any) {
      error = e.message || 'Bulk action failed';
    }
  }

  async function handleRowAction(id: string, action: string) {
    if (action === 'show_topics') {
      const msg = messages.find(m => m.id === id);
      if (msg) {
        topicsEmail = msg.from_address;
        topicsName = msg.from_name || null;
      }
      return;
    }
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
      if (action === 'archive') {
        feedback.success('Archived', { undo: async () => { await api.messages.batch([id], 'unarchive'); await loadMessages(); } });
      } else if (action === 'delete') {
        feedback.success('Deleted');
      } else if (action === 'star') {
        feedback.success('Starred');
      } else if (action === 'mark_read') {
        feedback.success('Marked as read');
      } else if (action === 'mark_unread') {
        feedback.success('Marked as unread');
      }
    } catch (e: any) {
      error = e.message || 'Action failed';
    }
  }

  async function handleSnooze(id: string, snoozeUntil: number) {
    try {
      await api.messages.snooze([id], snoozeUntil);
      feedback.success('Email snoozed');
      await loadMessages();
    } catch (e: any) {
      error = e.message || 'Snooze failed';
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

  async function handleSnoozeFocused() {
    if (focusedIndex < 0 || focusedIndex >= messages.length) return;
    const msg = messages[focusedIndex];
    // Snooze for 1 day by default (keyboard shortcut)
    const snoozeUntil = Math.floor(Date.now() / 1000) + 86400;
    await handleSnooze(msg.id, snoozeUntil);
  }

  async function handleMuteFocused() {
    if (focusedIndex < 0 || focusedIndex >= messages.length) return;
    const msg = messages[focusedIndex];
    const threadId = msg.thread_id || msg.id;
    try {
      await api.mutedThreads.mute(threadId);
      feedback.success('Thread muted');
      mutedThreadIds = new Set([...mutedThreadIds, threadId]);
    } catch (e: any) {
      error = e.message || 'Mute failed';
    }
  }

  // Helper functions for shortcut help overlay
  function groupShortcutsByMode(shortcuts: any[]): Record<string, any[]> {
    const grouped: Record<string, any[]> = {};
    shortcuts.forEach((sc) => {
      if (!grouped[sc.mode]) grouped[sc.mode] = [];
      grouped[sc.mode].push(sc);
    });
    return grouped;
  }

  function formatModeTitle(mode: string): string {
    const titles: Record<string, string> = {
      inbox: 'Inbox',
      thread: 'Thread View',
      compose: 'Compose',
      chat: 'Chat',
      settings: 'Settings',
      global: 'General',
    };
    return titles[mode] || mode.charAt(0).toUpperCase() + mode.slice(1);
  }

  // Register inbox keyboard shortcuts via centralized keyboard manager
  $effect(() => {
    currentMode.set('inbox');
    registerShortcuts([
      // Help overlay
      { key: '?', mode: 'inbox', action: () => { showShortcutHelp = !showShortcutHelp; }, description: 'Toggle shortcut help' },
      { key: 'Escape', mode: 'inbox', action: () => { showShortcutHelp = false; }, description: 'Close help overlay' },

      // Search
      { key: '/', mode: 'inbox', action: () => { const el = document.getElementById('topnav-search-input'); if (el) { el.focus(); (el as HTMLInputElement).select(); } }, description: 'Focus search' },

      // Compose
      { key: 'c', mode: 'inbox', action: () => { showCompose = true; }, description: 'Compose new email' },

      // Navigation
      { key: 'j', mode: 'inbox', action: () => { if (messages.length > 0) { focusedIndex = Math.min(focusedIndex + 1, messages.length - 1); scrollFocusedIntoView(); } }, description: 'Move to next message' },
      { key: 'k', mode: 'inbox', action: () => { if (messages.length > 0) { focusedIndex = Math.max(focusedIndex - 1, 0); scrollFocusedIntoView(); } }, description: 'Move to previous message' },
      { key: 'o', mode: 'inbox', action: () => { if (focusedIndex >= 0 && focusedIndex < messages.length) handleMessageClick(messages[focusedIndex].id); }, description: 'Open focused message' },
      { key: 'Enter', mode: 'inbox', action: () => { if (focusedIndex >= 0 && focusedIndex < messages.length) handleMessageClick(messages[focusedIndex].id); }, description: 'Open focused message' },

      // Actions on focused message
      { key: 'e', mode: 'inbox', action: () => { if (focusedIndex >= 0 && focusedIndex < messages.length) { const msg = messages[focusedIndex]; handleRowAction(msg.id, 'archive'); if (focusedIndex >= messages.length - 1) focusedIndex = Math.max(0, messages.length - 2); } }, description: 'Archive focused message' },
      { key: '#', mode: 'inbox', action: () => { if (focusedIndex >= 0 && focusedIndex < messages.length) { const msg = messages[focusedIndex]; handleRowAction(msg.id, 'delete'); if (focusedIndex >= messages.length - 1) focusedIndex = Math.max(0, messages.length - 2); } }, description: 'Delete focused message' },
      { key: 's', mode: 'inbox', action: () => { if (focusedIndex >= 0 && focusedIndex < messages.length) handleRowAction(messages[focusedIndex].id, 'star'); }, description: 'Star / unstar focused message' },
      { key: 'U', mode: 'inbox', shift: true, action: () => { if (focusedIndex >= 0 && focusedIndex < messages.length) handleRowAction(messages[focusedIndex].id, 'mark_unread'); }, description: 'Mark focused message unread' },

      // New shortcuts
      { key: 'b', mode: 'inbox', action: () => { handleSnoozeFocused(); }, description: 'Snooze focused message' },
      { key: 'm', mode: 'inbox', action: () => { handleMuteFocused(); }, description: 'Mute focused thread' },

      // Go-to chords
      { key: 'g+i', mode: 'inbox', action: () => push('/'), description: 'Go to Inbox' },
      { key: 'g+s', mode: 'inbox', action: () => push('/sent'), description: 'Go to Sent' },
      { key: 'g+d', mode: 'inbox', action: () => push('/drafts'), description: 'Go to Drafts' },
    ]);
  });

  $effect(() => {
    loadMessages();
    loadMutedThreads();
    loadNotificationPrefs();
    wsClient.connect();
    initNotifications();
    const offNewEmail = wsClient.on('NewEmail', (evt: any) => {
      feedback.info('New email received');
      showNotification('New email', { body: `From: ${evt.from || 'Unknown'}` });
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
      <MessageList {messages} onclick={handleMessageClick} bind:selectedIds onaction={handleRowAction} onsnooze={handleSnooze} {focusedIndex} {mutedThreadIds} />
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


{#if showCompose}
  <ComposeModal
    context={{ mode: 'new', accountId: activeAccountId }}
    onclose={() => (showCompose = false)}
    onsent={loadMessages}
  />
{/if}

{#if showShortcutHelp}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center"
    style="background: var(--iris-color-overlay);"
    onclick={() => showShortcutHelp = false}
    onkeydown={(e) => { if (e.key === 'Escape') showShortcutHelp = false; }}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="w-[520px] max-h-[80vh] overflow-y-auto rounded-xl p-6"
      style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);"
      onclick={(e) => e.stopPropagation()}
    >
      <div class="flex items-center justify-between mb-4">
        <h2 class="text-base font-semibold" style="color: var(--iris-color-text);">Keyboard Shortcuts</h2>
        <button
          class="p-1 rounded text-sm"
          style="color: var(--iris-color-text-faint);"
          onclick={() => showShortcutHelp = false}
        >&times;</button>
      </div>

      <div class="space-y-4">
        {#each Object.entries(groupShortcutsByMode(getAllShortcuts())) as [mode, shortcuts]}
          <div>
            <h3 class="text-xs font-semibold uppercase tracking-wider mb-2" style="color: var(--iris-color-primary);">{formatModeTitle(mode)}</h3>
            <div class="grid grid-cols-2 gap-x-8 gap-y-1">
              {#each shortcuts as sc}
                <div class="shortcut-row">
                  {#each sc.key.split(' / ') as keyPart}
                    <kbd>{keyPart}</kbd>
                  {/each}
                </div>
                <div class="shortcut-label">{sc.description}</div>
              {/each}
            </div>
          </div>
        {/each}
      </div>
    </div>
  </div>
{/if}

{#if showSpamDialog}
  <SpamDialog
    senderEmail={spamTargetEmail}
    messageIds={[spamTargetId]}
    onconfirm={handleReportSpam}
    onclose={() => { showSpamDialog = false; spamTargetId = ''; spamTargetEmail = ''; }}
  />
{/if}

{#if topicsEmail}
  <ContactTopicsPanel
    email={topicsEmail}
    name={topicsName}
    onclose={() => { topicsEmail = null; topicsName = null; }}
  />
{/if}

<style>
  .shortcut-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 0;
    font-size: 13px;
    color: var(--iris-color-text-muted);
  }
  .shortcut-label {
    display: flex;
    align-items: center;
    padding: 4px 0;
    font-size: 13px;
    color: var(--iris-color-text-faint);
  }
  .shortcut-row :global(kbd) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 24px;
    height: 22px;
    padding: 0 6px;
    font-family: 'SF Mono', 'Cascadia Mono', 'Menlo', monospace;
    font-size: 11px;
    font-weight: 500;
    border-radius: 4px;
    background: var(--iris-color-bg-surface);
    border: 1px solid var(--iris-color-border);
    color: var(--iris-color-text);
    line-height: 1;
  }
</style>
