<script lang="ts">
  import { api } from '../lib/api';
  import SettingsGeneral from '../components/settings/SettingsGeneral.svelte';
  import SettingsAppearance from '../components/settings/SettingsAppearance.svelte';
  import SettingsAI from '../components/settings/SettingsAI.svelte';
  import SettingsCompose from '../components/settings/SettingsCompose.svelte';
  import SettingsOrganization from '../components/settings/SettingsOrganization.svelte';
  import SettingsSecurity from '../components/settings/SettingsSecurity.svelte';
  import { irisFade } from '$lib/transitions';

  type TabId = 'general' | 'appearance' | 'ai' | 'compose' | 'organization' | 'security';

  const tabs: { id: TabId; label: string }[] = [
    { id: 'general', label: 'General' },
    { id: 'appearance', label: 'Appearance' },
    { id: 'ai', label: 'AI' },
    { id: 'compose', label: 'Compose' },
    { id: 'organization', label: 'Organization' },
    { id: 'security', label: 'Security' },
  ];

  function getTabFromHash(): TabId {
    const hash = window.location.hash;
    const match = hash.match(/^#\/settings\/(\w+)/);
    if (match) {
      const candidate = match[1] as TabId;
      if (tabs.some(t => t.id === candidate)) return candidate;
    }
    return 'general';
  }

  let activeTab = $state<TabId>(getTabFromHash());
  let accounts = $state<any[]>([]);

  function setTab(tab: TabId) {
    activeTab = tab;
    // Update hash without triggering navigation (svelte-spa-router uses hashchange)
    const newHash = `#/settings/${tab}`;
    history.replaceState(null, '', newHash);
  }

  // Listen for hash changes (browser back/forward)
  $effect(() => {
    function onHashChange() {
      activeTab = getTabFromHash();
    }
    window.addEventListener('hashchange', onHashChange);
    return () => window.removeEventListener('hashchange', onHashChange);
  });

  // Load accounts (shared state passed to tabs)
  $effect(() => {
    async function loadAccounts() {
      try {
        accounts = await api.accounts.list();
      } catch { accounts = []; }
    }
    loadAccounts();
  });
</script>

<div class="settings-layout">
  <!-- Sidebar tabs (desktop) / Top tabs (mobile) -->
  <!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
  <nav class="settings-nav" role="tablist" aria-label="Settings sections">
    {#each tabs as tab}
      <button
        role="tab"
        aria-selected={activeTab === tab.id}
        class="settings-tab"
        class:settings-tab--active={activeTab === tab.id}
        onclick={() => setTab(tab.id)}
      >
        {tab.label}
      </button>
    {/each}
  </nav>

  <!-- Tab content -->
  <div class="settings-content" role="tabpanel">
    {#key activeTab}
      <div transition:irisFade>
        {#if activeTab === 'general'}
          <SettingsGeneral bind:accounts />
        {:else if activeTab === 'appearance'}
          <SettingsAppearance />
        {:else if activeTab === 'ai'}
          <SettingsAI />
        {:else if activeTab === 'compose'}
          <SettingsCompose bind:accounts />
        {:else if activeTab === 'organization'}
          <SettingsOrganization />
        {:else if activeTab === 'security'}
          <SettingsSecurity bind:accounts />
        {/if}
      </div>
    {/key}
  </div>
</div>

<style>
  .settings-layout {
    display: flex;
    max-width: 900px;
    margin: 0 auto;
    padding: 48px 24px;
    gap: 32px;
    min-height: calc(100vh - 56px);
  }

  .settings-nav {
    display: flex;
    flex-direction: column;
    width: 200px;
    flex-shrink: 0;
    gap: 4px;
    position: sticky;
    top: 80px;
    align-self: flex-start;
  }

  .settings-tab {
    display: block;
    width: 100%;
    text-align: left;
    padding: 8px 16px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    color: var(--iris-color-text-faint);
    background: transparent;
    border: none;
    cursor: pointer;
    transition: color 200ms ease, background 200ms ease;
  }

  .settings-tab:hover {
    color: var(--iris-color-text);
    background: var(--iris-color-bg-surface);
  }

  .settings-tab--active {
    color: var(--iris-color-primary);
    background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent);
    border-left: 3px solid var(--iris-color-primary);
    padding-left: 13px;
  }

  .settings-tab--active:hover {
    color: var(--iris-color-primary);
    background: color-mix(in srgb, var(--iris-color-primary) 15%, transparent);
  }

  .settings-content {
    flex: 1;
    min-width: 0;
  }

  /* Mobile: horizontal top tabs */
  @media (max-width: 768px) {
    .settings-layout {
      flex-direction: column;
      padding: 24px 16px;
      gap: 24px;
    }

    .settings-nav {
      flex-direction: row;
      width: 100%;
      position: static;
      overflow-x: auto;
      gap: 0;
      border-bottom: 1px solid var(--iris-color-border);
      padding-bottom: 0;
      -webkit-overflow-scrolling: touch;
      scrollbar-width: none;
    }

    .settings-nav::-webkit-scrollbar {
      display: none;
    }

    .settings-tab {
      flex-shrink: 0;
      text-align: center;
      padding: 10px 16px;
      border-radius: 0;
      white-space: nowrap;
      border-left: none;
      border-bottom: 2px solid transparent;
      padding-left: 16px;
    }

    .settings-tab--active {
      border-left: none;
      border-bottom: 2px solid var(--iris-color-primary);
      background: transparent;
      padding-left: 16px;
    }

    .settings-tab--active:hover {
      background: transparent;
    }
  }
</style>
