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
  import Toast from '../components/Toast.svelte';
  import SpamDialog from '../components/SpamDialog.svelte';
  import ContactTopicsPanel from '../components/contacts/ContactTopicsPanel.svelte';


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
  let mutedThreadIds = $state(new Set<string>());
  let topicsEmail = $state<string | null>(null);
  let topicsName = $state<string | null>(null);
  let notificationPrefs = $state<Record<string, boolean>>({});

  // Keyboard navigation state
  let focusedIndex = $state(-1);
  let showShortcutHelp = $state(false);
  let pendingGChord = $state(false);
  let gChordTimer: ReturnType<typeof setTimeout> | null = null;

  function isInputFocused(): boolean {
    const el = document.activeElement;
    if (!el) return false;
    const tag = el.tagName.toLowerCase();
    return tag === 'input' || tag === 'textarea' || tag === 'select' || (el as HTMLElement).isContentEditable;
  }

  function focusSearch() {
    const input = document.getElementById('topnav-search-input') as HTMLInputElement | null;
    if (input) {
      input.focus();
      input.select();
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    // Always allow shortcut help toggle
    if (e.key === '?' && !e.metaKey && !e.ctrlKey && !e.altKey) {
      if (isInputFocused()) return;
      e.preventDefault();
      showShortcutHelp = !showShortcutHelp;
      return;
    }

    // Close help overlay on Escape
    if (e.key === 'Escape' && showShortcutHelp) {
      e.preventDefault();
      showShortcutHelp = false;
      return;
    }

    // Cmd+K / Ctrl+K — focus search (works even in inputs)
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      focusSearch();
      return;
    }

    // Skip all other shortcuts if user is typing
    if (isInputFocused()) return;

    // Handle "g" chord
    if (pendingGChord) {
      pendingGChord = false;
      if (gChordTimer) { clearTimeout(gChordTimer); gChordTimer = null; }
      switch (e.key) {
        case 'i': e.preventDefault(); push('/'); return;
        case 's': e.preventDefault(); push('/sent'); return;
        case 'd': e.preventDefault(); push('/drafts'); return;
      }
      // Unrecognized second key — fall through
      return;
    }

    if (e.key === 'g' && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
      e.preventDefault();
      pendingGChord = true;
      gChordTimer = setTimeout(() => { pendingGChord = false; }, 1000);
      return;
    }

    // "/" — focus search
    if (e.key === '/' && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      focusSearch();
      return;
    }

    // "c" — compose
    if (e.key === 'c' && !e.metaKey && !e.ctrlKey && !e.shiftKey) {
      e.preventDefault();
      showCompose = true;
      return;
    }

    // j/k — navigate list
    if (e.key === 'j') {
      e.preventDefault();
      if (messages.length > 0) {
        focusedIndex = Math.min(focusedIndex + 1, messages.length - 1);
        scrollFocusedIntoView();
      }
      return;
    }
    if (e.key === 'k') {
      e.preventDefault();
      if (messages.length > 0) {
        focusedIndex = Math.max(focusedIndex - 1, 0);
        scrollFocusedIntoView();
      }
      return;
    }

    // Enter/o — open focused message
    if ((e.key === 'Enter' || e.key === 'o') && focusedIndex >= 0 && focusedIndex < messages.length) {
      e.preventDefault();
      handleMessageClick(messages[focusedIndex].id);
      return;
    }

    // Actions on focused message
    if (focusedIndex >= 0 && focusedIndex < messages.length) {
      const msg = messages[focusedIndex];

      // e — archive
      if (e.key === 'e' && !e.metaKey && !e.ctrlKey) {
        e.preventDefault();
        handleRowAction(msg.id, 'archive');
        // Adjust focus if we removed the last item
        if (focusedIndex >= messages.length - 1) {
          focusedIndex = Math.max(0, messages.length - 2);
        }
        return;
      }

      // # — delete
      if (e.key === '#') {
        e.preventDefault();
        handleRowAction(msg.id, 'delete');
        if (focusedIndex >= messages.length - 1) {
          focusedIndex = Math.max(0, messages.length - 2);
        }
        return;
      }

      // s — star/unstar
      if (e.key === 's' && !e.metaKey && !e.ctrlKey && !e.shiftKey) {
        e.preventDefault();
        handleRowAction(msg.id, 'star');
        return;
      }

      // Shift+u — mark unread
      if (e.key === 'U' || (e.key === 'u' && e.shiftKey)) {
        e.preventDefault();
        handleRowAction(msg.id, 'mark_unread');
        return;
      }
    }
  }

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
    try {
      await api.messages.batch([...selectedIds], action);
      selectedIds = new Set();
      await loadMessages();
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
    } catch (e: any) {
      error = e.message || 'Action failed';
    }
  }

  async function handleSnooze(id: string, snoozeUntil: number) {
    try {
      await api.messages.snooze([id], snoozeUntil);
      toastMessage = 'Email snoozed';
      toastVisible = true;
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

  $effect(() => {
    loadMessages();
    loadMutedThreads();
    loadNotificationPrefs();
    wsClient.connect();
    initNotifications();
    const offNewEmail = wsClient.on('NewEmail', (evt: any) => {
      toastMessage = `New email received`;
      toastVisible = true;
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

<svelte:window onkeydown={handleKeydown} />

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

<Toast message={toastMessage} visible={toastVisible} ondismiss={() => toastVisible = false} />

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

      <div class="grid grid-cols-2 gap-x-8 gap-y-1">
        <!-- Navigation -->
        <div class="col-span-2 mt-2 mb-1">
          <h3 class="text-xs font-semibold uppercase tracking-wider" style="color: var(--iris-color-primary);">Navigation</h3>
        </div>
        <div class="shortcut-row"><kbd>j</kbd> / <kbd>k</kbd></div>
        <div class="shortcut-label">Move down / up</div>
        <div class="shortcut-row"><kbd>Enter</kbd> or <kbd>o</kbd></div>
        <div class="shortcut-label">Open message</div>
        <div class="shortcut-row"><kbd>/</kbd> or <kbd>{navigator.platform.includes('Mac') ? '\u2318' : 'Ctrl'}+K</kbd></div>
        <div class="shortcut-label">Focus search</div>
        <div class="shortcut-row"><kbd>g</kbd> then <kbd>i</kbd></div>
        <div class="shortcut-label">Go to Inbox</div>
        <div class="shortcut-row"><kbd>g</kbd> then <kbd>s</kbd></div>
        <div class="shortcut-label">Go to Sent</div>
        <div class="shortcut-row"><kbd>g</kbd> then <kbd>d</kbd></div>
        <div class="shortcut-label">Go to Drafts</div>

        <!-- Actions -->
        <div class="col-span-2 mt-3 mb-1">
          <h3 class="text-xs font-semibold uppercase tracking-wider" style="color: var(--iris-color-primary);">Actions (Inbox)</h3>
        </div>
        <div class="shortcut-row"><kbd>c</kbd></div>
        <div class="shortcut-label">Compose new email</div>
        <div class="shortcut-row"><kbd>e</kbd></div>
        <div class="shortcut-label">Archive</div>
        <div class="shortcut-row"><kbd>#</kbd></div>
        <div class="shortcut-label">Delete</div>
        <div class="shortcut-row"><kbd>s</kbd></div>
        <div class="shortcut-label">Star / unstar</div>
        <div class="shortcut-row"><kbd>Shift+U</kbd></div>
        <div class="shortcut-label">Mark unread</div>

        <!-- Thread -->
        <div class="col-span-2 mt-3 mb-1">
          <h3 class="text-xs font-semibold uppercase tracking-wider" style="color: var(--iris-color-primary);">Thread View</h3>
        </div>
        <div class="shortcut-row"><kbd>r</kbd></div>
        <div class="shortcut-label">Reply</div>
        <div class="shortcut-row"><kbd>a</kbd></div>
        <div class="shortcut-label">Reply all</div>
        <div class="shortcut-row"><kbd>f</kbd></div>
        <div class="shortcut-label">Forward</div>
        <div class="shortcut-row"><kbd>e</kbd></div>
        <div class="shortcut-label">Archive thread</div>
        <div class="shortcut-row"><kbd>s</kbd></div>
        <div class="shortcut-label">Star thread</div>
        <div class="shortcut-row"><kbd>#</kbd></div>
        <div class="shortcut-label">Delete thread</div>
        <div class="shortcut-row"><kbd>u</kbd> or <kbd>Esc</kbd></div>
        <div class="shortcut-label">Back to inbox</div>

        <!-- General -->
        <div class="col-span-2 mt-3 mb-1">
          <h3 class="text-xs font-semibold uppercase tracking-wider" style="color: var(--iris-color-primary);">General</h3>
        </div>
        <div class="shortcut-row"><kbd>?</kbd></div>
        <div class="shortcut-label">Show this help</div>
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
