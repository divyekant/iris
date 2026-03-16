<script lang="ts">
  import type { Snippet } from 'svelte';
  import TopNav from './TopNav.svelte';
  import ChatPanel from './ChatPanel.svelte';
  import Toast from './Toast.svelte';
  import CommandPalette from './shared/CommandPalette.svelte';
  import { api } from '../lib/api';
  import { wsClient } from '../lib/ws';
  import { handleGlobalKeydown, registerShortcut, currentMode } from '$lib/keyboard';
  import { togglePalette, registerCommands } from '$lib/commands';
  import { push } from 'svelte-spa-router';
  import { currentThreadContext } from '$lib/threadContext';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();

  let accounts: any[] = $state([]);
  let activeAccountId: string | null = $state(null);
  let chatOpen = $state(false);
  let syncStatus: string | null = $state(null);
  let syncError = $state(false);
  let unreadCount = $state(0);

  $effect(() => {
    // Register Cmd+K shortcut
    registerShortcut({
      key: 'k',
      mode: 'global',
      meta: true,
      action: togglePalette,
      description: 'Open command palette',
    });

    // Register core navigation commands
    registerCommands([
      { id: 'nav-inbox', label: 'Go to Inbox', category: 'Navigation', keywords: ['home'], action: () => push('/'), shortcut: 'g → i' },
      { id: 'nav-sent', label: 'Go to Sent', category: 'Navigation', keywords: ['sent'], action: () => push('/sent'), shortcut: 'g → s' },
      { id: 'nav-drafts', label: 'Go to Drafts', category: 'Navigation', keywords: ['draft'], action: () => push('/drafts'), shortcut: 'g → d' },
      { id: 'nav-settings', label: 'Open Settings', category: 'Navigation', keywords: ['preferences', 'config'], action: () => push('/settings') },
      { id: 'nav-search', label: 'Focus Search', category: 'Navigation', keywords: ['find'], action: () => { const el = document.getElementById('topnav-search-input'); if (el) { el.focus(); (el as HTMLInputElement).select(); } }, shortcut: '/' },
    ]);
  });

  $effect(() => {
    loadAccounts();
    loadMessages();
    const unsub1 = wsClient.on('SyncStatus', (evt: any) => {
      syncStatus = evt.data?.message || 'Syncing...';
      syncError = false;
    });
    const unsub2 = wsClient.on('SyncComplete', () => {
      syncStatus = null;
      syncError = false;
      loadMessages();
    });
    const unsub3 = wsClient.on('NewEmail', () => {
      loadMessages();
    });
    return () => { unsub1(); unsub2(); unsub3(); };
  });

  async function loadAccounts() {
    try {
      const res = await api.accounts.list();
      accounts = res;
      if (!activeAccountId && accounts.length > 0) {
        activeAccountId = accounts[0].id;
      }
    } catch {}
  }

  async function loadMessages() {
    try {
      const res = await api.messages.list({ account_id: activeAccountId || undefined, limit: 1 });
      unreadCount = res.unread_count || 0;
    } catch {}
  }

  function handleAccountSwitch(id: string | null) {
    activeAccountId = id;
    window.dispatchEvent(new CustomEvent('account-switch', { detail: { accountId: id } }));
  }

  function handleCompose() {
    window.dispatchEvent(new CustomEvent('open-compose', { detail: { mode: 'new', accountId: activeAccountId } }));
  }
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<CommandPalette />

<div class="flex flex-col h-screen" style="background: var(--iris-color-bg);">
  <TopNav
    {accounts}
    {activeAccountId}
    {unreadCount}
    {syncStatus}
    {syncError}
    chatOpen={chatOpen}
    onchatToggle={() => chatOpen = !chatOpen}
    oncompose={handleCompose}
    onaccountSwitch={handleAccountSwitch}
  />

  <div class="flex flex-1 overflow-hidden">
    <main class="flex-1 overflow-y-auto">
      {@render children()}
    </main>

    {#if chatOpen}
      <ChatPanel open={chatOpen} onclose={() => chatOpen = false} threadContext={$currentThreadContext} />
    {/if}
  </div>
</div>

<Toast />

<!-- Mode indicator -->
{#if $currentMode && $currentMode !== 'global'}
  <div
    style="position: fixed; bottom: 8px; left: 8px; z-index: 40; background: var(--iris-color-bg-elevated); color: var(--iris-color-text-faint); border: 1px solid var(--iris-color-border); border-radius: var(--iris-radius-sm); padding: 2px 8px; font-size: 10px; text-transform: uppercase; letter-spacing: 0.5px; pointer-events: none;"
  >
    {$currentMode}
  </div>
{/if}
