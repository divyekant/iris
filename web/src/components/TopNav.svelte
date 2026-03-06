<script lang="ts">
  import { push, location } from 'svelte-spa-router';
  import { api } from '../lib/api';

  interface Props {
    accounts?: any[];
    activeAccountId?: string | null;
    unreadCount?: number;
    syncStatus?: string | null;
    syncError?: boolean;
    chatOpen?: boolean;
    onchatToggle?: () => void;
    oncompose?: () => void;
    onaccountSwitch?: (accountId: string | null) => void;
  }

  let {
    accounts = [],
    activeAccountId = null,
    unreadCount = 0,
    syncStatus = null,
    syncError = false,
    chatOpen = false,
    onchatToggle,
    oncompose,
    onaccountSwitch,
  }: Props = $props();

  let accountDropdownOpen = $state(false);
  let overflowOpen = $state(false);
  let isDark = $state(!document.documentElement.hasAttribute('data-brand'));

  function toggleTheme() {
    isDark = !isDark;
    if (isDark) {
      document.documentElement.removeAttribute('data-brand');
      localStorage.setItem('iris-theme', 'dark');
    } else {
      document.documentElement.setAttribute('data-brand', 'light');
      localStorage.setItem('iris-theme', 'light');
    }
    api.config.setTheme(isDark ? 'dark' : 'light').catch(() => {});
  }

  const navItems = [
    { path: '/', label: 'Inbox', icon: 'inbox' },
    { path: '/search', label: 'Search', icon: 'search' },
    { path: '/drafts', label: 'Drafts', icon: 'file-text' },
  ];

  const overflowItems = [
    { path: '/starred', label: 'Starred', icon: 'star' },
    { path: '/archive', label: 'Archive', icon: 'archive' },
    { path: '/trash', label: 'Trash', icon: 'trash-2' },
    { divider: true },
    { path: '/settings', label: 'Settings', icon: 'settings' },
  ];

  function isActive(path: string): boolean {
    if (path === '/') return $location === '/' || $location === '';
    return $location.startsWith(path);
  }

  function navigate(path: string) {
    push(path);
    overflowOpen = false;
  }

  function selectAccount(id: string | null) {
    onaccountSwitch?.(id);
    accountDropdownOpen = false;
  }

  const activeAccount = $derived(
    accounts.find(a => a.id === activeAccountId) || accounts[0]
  );

  function handleClickOutside(e: MouseEvent) {
    accountDropdownOpen = false;
    overflowOpen = false;
  }
</script>

<svelte:window onclick={handleClickOutside} />

<nav
  class="flex items-center h-14 px-6 gap-4"
  style="background-color: var(--iris-color-bg-elevated); border-bottom: 1px solid var(--iris-color-border-subtle);"
>
  <!-- Logo -->
  <span
    class="text-xl font-bold cursor-pointer select-none"
    style="color: var(--iris-color-primary);"
    onclick={() => push('/')}
  >Iris</span>

  <!-- Account Switcher / Add Account -->
  {#if accounts.length === 0}
    <button
      class="flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded-md"
      style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
      onclick={() => push('/setup')}
    >
      <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 5v14M5 12h14"/></svg>
      Add Account
    </button>
  {:else}
    <div class="relative">
      <button
        class="flex items-center gap-1.5 px-2.5 py-1 text-xs rounded-md"
        style="background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent);
               border: 1px solid var(--iris-color-border);"
        onclick={(e) => { e.stopPropagation(); accountDropdownOpen = !accountDropdownOpen; }}
      >
        <span
          class="w-5 h-5 rounded-full flex items-center justify-center text-[10px] font-semibold"
          style="background: color-mix(in srgb, var(--iris-color-primary) 20%, transparent);
                 color: var(--iris-color-primary);"
        >{activeAccount?.email?.[0]?.toUpperCase() || '?'}</span>
        <span style="color: var(--iris-color-text);">{activeAccount?.email || 'No account'}</span>
        <svg class="w-3.5 h-3.5" style="color: var(--iris-color-text-faint);" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"/></svg>
      </button>

      {#if accountDropdownOpen}
        <div
          class="absolute top-full left-0 mt-1 w-72 rounded-xl p-2 z-50"
          style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);"
          onclick={(e) => e.stopPropagation()}
        >
          <p class="text-[11px] font-medium px-2 py-1" style="color: var(--iris-color-text-faint);">Switch account</p>
          {#each accounts as account}
            <button
              class="flex items-center gap-2.5 w-full px-2.5 py-2 rounded-lg text-left"
              style={account.id === activeAccountId
                ? 'background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent);'
                : ''}
              onclick={() => selectAccount(account.id)}
            >
              <span
                class="w-7 h-7 rounded-full flex items-center justify-center text-xs font-semibold shrink-0"
                style="background: color-mix(in srgb, var(--iris-color-primary) 20%, transparent);
                       color: var(--iris-color-primary);"
              >{account.email?.[0]?.toUpperCase()}</span>
              <div class="flex-1 min-w-0">
                <p class="text-[13px] truncate" style="color: var(--iris-color-text); font-weight: {account.id === activeAccountId ? '500' : '400'};">{account.email}</p>
                <p class="text-[11px]" style="color: var(--iris-color-text-faint);">{account.provider || 'IMAP'}</p>
              </div>
              {#if account.id === activeAccountId}
                <svg class="w-3.5 h-3.5 shrink-0" style="color: var(--iris-color-primary);" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7"/></svg>
              {/if}
            </button>
          {/each}
          <button
            class="flex items-center gap-2.5 w-full px-2.5 py-2 rounded-lg text-left"
            onclick={() => selectAccount(null)}
          >
            <svg class="w-4 h-4" style="color: var(--iris-color-text-muted);" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6.5v0M12 12v0M12 17.5v0M4 12h16"/></svg>
            <span class="text-[13px]" style="color: var(--iris-color-text-muted);">All accounts</span>
          </button>
          <div class="my-1" style="border-top: 1px solid var(--iris-color-border);"></div>
          <button
            class="flex items-center gap-2.5 w-full px-2.5 py-2 rounded-lg text-left"
            onclick={() => { accountDropdownOpen = false; push('/setup'); }}
          >
            <svg class="w-4 h-4" style="color: var(--iris-color-text-faint);" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 5v14M5 12h14"/></svg>
            <span class="text-[13px]" style="color: var(--iris-color-text-faint);">Add account</span>
          </button>
        </div>
      {/if}
    </div>
  {/if}

  <!-- Nav Items (only show when accounts exist) -->
  <div class="flex items-center gap-0.5">
    {#each navItems as item}
      <button
        class="flex items-center gap-1.5 px-2.5 h-8 rounded-lg text-[13px] font-medium transition-colors"
        style={isActive(item.path)
          ? 'background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent); color: var(--iris-color-primary);'
          : 'color: var(--iris-color-text-muted);'}
        onclick={() => navigate(item.path)}
      >{item.label}</button>
    {/each}

    <!-- Overflow Menu -->
    <div class="relative">
      <button
        class="flex items-center gap-1.5 px-2.5 h-8 rounded-lg text-[13px] font-medium"
        style="color: var(--iris-color-text-muted);"
        onclick={(e) => { e.stopPropagation(); overflowOpen = !overflowOpen; }}
      >More</button>

      {#if overflowOpen}
        <div
          class="absolute top-full left-0 mt-1 w-48 rounded-xl p-1.5 z-50"
          style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);"
          onclick={(e) => e.stopPropagation()}
        >
          {#each overflowItems as item}
            {#if item.divider}
              <div class="my-1" style="border-top: 1px solid var(--iris-color-border);"></div>
            {:else}
              <button
                class="flex items-center gap-2 w-full px-2.5 h-9 rounded-lg text-[13px] text-left"
                style="color: var(--iris-color-text-muted);"
                onclick={() => navigate(item.path!)}
              >{item.label}</button>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  </div>

  <!-- Spacer -->
  <div class="flex-1"></div>

  <!-- Sync Status -->
  {#if syncStatus}
    <span
      class="flex items-center gap-1.5 px-3 py-1 rounded-full text-[11px] font-medium"
      style={syncError
        ? 'background: color-mix(in srgb, var(--iris-color-error) 10%, transparent); color: var(--iris-color-error);'
        : 'background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent); color: var(--iris-color-primary);'}
    >{syncStatus}</span>
  {/if}

  <!-- Theme Toggle -->
  <button
    class="flex items-center justify-center w-8 h-8 rounded-lg"
    style="background: var(--iris-color-bg-surface); border: 1px solid var(--iris-color-border); color: var(--iris-color-text-muted);"
    onclick={toggleTheme}
    title={isDark ? 'Switch to light mode' : 'Switch to dark mode'}
  >
    {#if isDark}
      <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><circle cx="12" cy="12" r="5" stroke-width="2"/><path stroke-width="2" stroke-linecap="round" d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42"/></svg>
    {:else}
      <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z"/></svg>
    {/if}
  </button>

  <!-- AI Chat Toggle -->
  <button
    class="flex items-center gap-1.5 px-3 h-8 rounded-lg text-[13px] font-medium"
    style={chatOpen
      ? 'background: var(--iris-color-primary); color: var(--iris-color-bg);'
      : 'background: var(--iris-color-bg-surface); color: var(--iris-color-text-muted); border: 1px solid var(--iris-color-border);'}
    onclick={onchatToggle}
  >AI Chat</button>

  <!-- Compose Button -->
  <button
    class="flex items-center gap-1.5 px-3.5 h-8 rounded-lg text-[13px] font-medium"
    style="background: var(--iris-color-bg-surface); color: var(--iris-color-text-muted); border: 1px solid var(--iris-color-border);"
    onclick={oncompose}
  >Compose</button>
</nav>
