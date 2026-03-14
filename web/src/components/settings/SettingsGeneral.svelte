<script lang="ts">
  import { api } from '../../lib/api';
  import { requestNotificationPermission, setEnabled as setNotificationsEnabled, getPermissionState } from '../../lib/notifications';
  import { Bell, BellOff } from 'lucide-svelte';
  import FormToggle from '../shared/FormToggle.svelte';
  import FormSelect from '../shared/FormSelect.svelte';

  type Theme = 'light' | 'dark' | 'system';

  let { accounts = $bindable([]) }: { accounts: any[] } = $props();

  let currentTheme = $state<Theme>('system');
  let loading = $state(true);

  // Undo send
  let undoSendDelay = $state('10');
  let undoSendSaving = $state(false);

  // Desktop notifications
  let notificationsEnabled = $state(localStorage.getItem('iris-notifications') !== 'false');
  let notificationPermission = $state(getPermissionState());

  // Per-account notification control
  let acctNotificationState = $state<Record<string, boolean>>({});
  let acctNotificationToggling = $state<Record<string, boolean>>({});

  const themes: { value: Theme; label: string; icon: string }[] = [
    { value: 'light', label: 'Light', icon: '\u{2600}\u{FE0F}' },
    { value: 'dark', label: 'Dark', icon: '\u{1F319}' },
    { value: 'system', label: 'System', icon: '\u{1F4BB}' },
  ];

  const undoSendOptions = [
    { value: '5', label: '5 seconds' },
    { value: '10', label: '10 seconds' },
    { value: '15', label: '15 seconds' },
    { value: '20', label: '20 seconds' },
    { value: '30', label: '30 seconds' },
  ];

  function applyTheme(theme: string) {
    if (theme === 'system') {
      const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
      if (!prefersDark) {
        document.documentElement.setAttribute('data-brand', 'light');
      } else {
        document.documentElement.removeAttribute('data-brand');
      }
    } else if (theme === 'light') {
      document.documentElement.setAttribute('data-brand', 'light');
    } else {
      // dark is the default (:root), remove attribute
      document.documentElement.removeAttribute('data-brand');
    }
    localStorage.setItem('iris-theme', theme);
  }

  async function setTheme(theme: Theme) {
    currentTheme = theme;
    applyTheme(theme);
    try {
      await api.config.setTheme(theme);
    } catch {
      // Silently fail — theme is already applied locally
    }
  }

  // notificationsEnabled is toggled by FormToggle's bind:checked.
  // This effect watches for changes and handles the async permission logic.
  let notifInitialized = $state(false);
  $effect(() => {
    const val = notificationsEnabled;
    if (!notifInitialized) return;
    if (val) {
      // Turning on — request permission
      requestNotificationPermission().then(granted => {
        notificationPermission = getPermissionState();
        if (!granted) {
          // Revert if permission denied
          notificationsEnabled = false;
        } else {
          setNotificationsEnabled(true);
        }
      });
    } else {
      // Turning off
      setNotificationsEnabled(false);
    }
  });

  async function saveUndoSendDelay() {
    undoSendSaving = true;
    try {
      const result = await api.config.setUndoSendDelay(parseInt(undoSendDelay));
      undoSendDelay = String(result.delay_seconds);
    } catch {
      // Silently fail
    } finally {
      undoSendSaving = false;
    }
  }

  async function loadAccountNotifications() {
    try {
      await Promise.all(accounts.map(async (account: any) => {
        try {
          const res = await api.notifications.get(account.id);
          acctNotificationState[account.id] = res.enabled;
        } catch {
          acctNotificationState[account.id] = true; // default enabled
        }
      }));
    } catch { /* ignore */ }
  }

  async function toggleAccountNotification(accountId: string) {
    if (acctNotificationToggling[accountId]) return;
    acctNotificationToggling[accountId] = true;
    const newValue = !acctNotificationState[accountId];
    try {
      const res = await api.notifications.set(accountId, newValue);
      acctNotificationState[accountId] = res.enabled;
    } catch {
      // revert on failure
    } finally {
      acctNotificationToggling[accountId] = false;
    }
  }

  // Watch undo send delay changes from FormSelect
  let prevUndoSendDelay = $state('10');
  $effect(() => {
    if (undoSendDelay !== prevUndoSendDelay) {
      prevUndoSendDelay = undoSendDelay;
      saveUndoSendDelay();
    }
  });

  // Load settings on mount
  $effect(() => {
    async function loadSettings() {
      try {
        const config = await api.config.get();
        currentTheme = (config.theme as Theme) || 'system';
        applyTheme(currentTheme);
      } catch {
        currentTheme = 'system';
      }

      try {
        const undoConfig = await api.config.getUndoSendDelay();
        undoSendDelay = String(undoConfig.delay_seconds);
        prevUndoSendDelay = undoSendDelay;
      } catch {
        undoSendDelay = '10';
        prevUndoSendDelay = '10';
      }

      await loadAccountNotifications();

      notifInitialized = true;
      loading = false;
    }
    loadSettings();
  });
</script>

<div class="space-y-8">
  <!-- Theme section -->
  <section>
    <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Appearance</h3>
    <div class="flex gap-3">
      {#each themes as theme}
        <button
          class="settings-theme-card flex-1 flex flex-col items-center gap-2 px-4 py-4 rounded-xl border-2 transition-colors"
          style="border-color: {currentTheme === theme.value ? 'var(--iris-color-primary)' : 'var(--iris-color-border-subtle)'}; background: {currentTheme === theme.value ? 'color-mix(in srgb, var(--iris-color-primary) 10%, transparent)' : 'transparent'}; color: {currentTheme === theme.value ? 'var(--iris-color-primary)' : 'var(--iris-color-text)'};"
          onclick={() => setTheme(theme.value)}
          disabled={loading}
        >
          <span class="text-2xl">{theme.icon}</span>
          <span class="text-sm font-medium">{theme.label}</span>
        </button>
      {/each}
    </div>

    <!-- Desktop Notifications -->
    <div class="mt-6 p-3 rounded-lg border" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface);">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm font-medium" style="color: var(--iris-color-text);">Desktop Notifications</p>
          <p class="text-xs" style="color: var(--iris-color-text-faint);">
            {#if notificationPermission === 'unsupported'}
              Not available in this browser
            {:else if notificationPermission === 'denied'}
              Blocked by browser — update in site settings
            {:else if notificationsEnabled && notificationPermission === 'granted'}
              Enabled — you'll see alerts for new emails
            {:else}
              Get notified when new emails arrive
            {/if}
          </p>
        </div>
        <FormToggle
          bind:checked={notificationsEnabled}
          disabled={notificationPermission === 'unsupported' || notificationPermission === 'denied'}
        />
      </div>
    </div>

    <!-- Per-account notification control -->
    {#if accounts.length > 0}
      <div class="mt-4 space-y-3">
        <p class="text-xs font-medium" style="color: var(--iris-color-text-muted);">Per-account notifications</p>
        {#each accounts as account}
          <div class="flex items-center justify-between p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
            <div class="flex items-center gap-3">
              {#if acctNotificationState[account.id] !== false}
                <Bell size={18} style="color: var(--iris-color-success);" />
              {:else}
                <BellOff size={18} style="color: var(--iris-color-text-faint);" />
              {/if}
              <div>
                <p class="text-sm font-medium" style="color: var(--iris-color-text);">{account.email}</p>
                <p class="text-xs" style="color: var(--iris-color-text-faint);">
                  {acctNotificationState[account.id] !== false ? 'Notifications enabled' : 'Notifications disabled'}
                </p>
              </div>
            </div>
            <button
              class="relative w-11 h-6 rounded-full transition-colors"
              style="background: {acctNotificationState[account.id] !== false ? 'var(--iris-color-success)' : 'var(--iris-color-border-subtle)'};"
              onclick={() => toggleAccountNotification(account.id)}
              disabled={acctNotificationToggling[account.id]}
              aria-label="Toggle notifications for {account.email}"
            >
              <span
                class="absolute top-0.5 left-0.5 w-5 h-5 rounded-full shadow transition-transform {acctNotificationState[account.id] !== false ? 'translate-x-5' : ''}"
                style="background: var(--iris-color-text);"
              ></span>
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </section>

  <!-- Undo Send section -->
  <section>
    <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Undo Send</h3>
    <div class="flex items-center justify-between">
      <div>
        <p class="text-sm font-medium" style="color: var(--iris-color-text);">Send delay</p>
        <p class="text-xs" style="color: var(--iris-color-text-faint);">Time to undo after clicking Send</p>
      </div>
      <div class="flex items-center gap-2">
        <FormSelect
          bind:value={undoSendDelay}
          options={undoSendOptions}
          disabled={undoSendSaving}
        />
        {#if undoSendSaving}
          <span class="text-xs" style="color: var(--iris-color-text-faint);">Saving...</span>
        {/if}
      </div>
    </div>
  </section>
</div>

<style>
  .settings-theme-card:hover {
    border-color: var(--iris-color-primary) !important;
  }
</style>
