<script lang="ts">
  import type { Snippet } from 'svelte';
  import TopNav from './TopNav.svelte';
  import ChatPanel from './ChatPanel.svelte';
  import Toast from './Toast.svelte';
  import { api } from '../lib/api';
  import { wsClient } from '../lib/ws';

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
      <ChatPanel open={chatOpen} onclose={() => chatOpen = false} />
    {/if}
  </div>
</div>

<Toast />
