<script lang="ts">
  import { push, location } from 'svelte-spa-router';
  import { api } from '../lib/api';

  let accounts = $state<any[]>([]);
  let activeAccountId = $state('');

  const navItems = [
    { path: '/', label: 'Inbox', icon: '\u{1F4E5}' },
    { path: '/setup', label: 'Add Account', icon: '\u{2795}' },
    { path: '/settings', label: 'Settings', icon: '\u{2699}\u{FE0F}' },
  ];

  async function loadAccounts() {
    try {
      accounts = await api.accounts.list();
    } catch { /* ignore */ }
  }

  function selectAccount(id: string) {
    activeAccountId = id;
    window.dispatchEvent(new CustomEvent('account-switch', { detail: { accountId: id } }));
    push('/');
  }

  $effect(() => {
    loadAccounts();
  });
</script>

<aside class="w-56 border-r border-gray-200 dark:border-gray-700 flex flex-col">
  <div class="p-4 border-b border-gray-200 dark:border-gray-700">
    <h1 class="text-xl font-bold">Iris</h1>
  </div>

  {#if accounts.length > 0}
    <div class="p-2 border-b border-gray-200 dark:border-gray-700">
      <p class="px-3 py-1 text-xs font-semibold text-gray-400 uppercase tracking-wider">Accounts</p>
      <button
        class="w-full text-left px-3 py-1.5 rounded text-sm transition-colors
               {activeAccountId === '' ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 font-medium' : 'hover:bg-gray-100 dark:hover:bg-gray-800 text-gray-600 dark:text-gray-400'}"
        onclick={() => selectAccount('')}
      >
        All Accounts
      </button>
      {#each accounts as account}
        <button
          class="w-full text-left px-3 py-1.5 rounded text-sm transition-colors truncate
                 {activeAccountId === account.id ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 font-medium' : 'hover:bg-gray-100 dark:hover:bg-gray-800 text-gray-600 dark:text-gray-400'}"
          onclick={() => selectAccount(account.id)}
          title={account.email}
        >
          {account.email}
        </button>
      {/each}
    </div>
  {/if}

  <nav class="flex-1 p-2 space-y-1">
    {#each navItems as item}
      <button
        class="w-full text-left px-3 py-2 rounded-lg text-sm transition-colors
               {$location === item.path ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 font-medium' : 'hover:bg-gray-100 dark:hover:bg-gray-800'}"
        onclick={() => push(item.path)}
      >
        <span class="mr-2">{item.icon}</span>
        {item.label}
      </button>
    {/each}
  </nav>
</aside>
