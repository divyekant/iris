<script lang="ts">
  import { api } from '../lib/api';

  type Theme = 'light' | 'dark' | 'system';

  let currentTheme = $state<Theme>('system');
  let loading = $state(true);

  const themes: { value: Theme; label: string; icon: string }[] = [
    { value: 'light', label: 'Light', icon: '\u{2600}\u{FE0F}' },
    { value: 'dark', label: 'Dark', icon: '\u{1F319}' },
    { value: 'system', label: 'System', icon: '\u{1F4BB}' },
  ];

  function applyTheme(theme: string) {
    const isDark =
      theme === 'dark' ||
      (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
    document.documentElement.classList.toggle('dark', isDark);
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

  // Load current theme on mount
  $effect(() => {
    async function loadTheme() {
      try {
        const config = await api.config.get();
        currentTheme = (config.theme as Theme) || 'system';
        applyTheme(currentTheme);
      } catch {
        // Default to system theme if API fails
        currentTheme = 'system';
      } finally {
        loading = false;
      }
    }
    loadTheme();
  });
</script>

<div class="max-w-2xl mx-auto py-12 px-6">
  <h2 class="text-2xl font-bold mb-8">Settings</h2>

  <div class="space-y-8">
    <!-- Theme section -->
    <section>
      <h3 class="text-sm font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-4">Appearance</h3>
      <div class="flex gap-3">
        {#each themes as theme}
          <button
            class="flex-1 flex flex-col items-center gap-2 px-4 py-4 rounded-xl border-2 transition-colors
                   {currentTheme === theme.value
                     ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20 text-blue-700 dark:text-blue-300'
                     : 'border-gray-200 dark:border-gray-700 hover:border-gray-300 dark:hover:border-gray-600'}"
            onclick={() => setTheme(theme.value)}
            disabled={loading}
          >
            <span class="text-2xl">{theme.icon}</span>
            <span class="text-sm font-medium">{theme.label}</span>
          </button>
        {/each}
      </div>
    </section>

    <!-- Placeholder for future settings -->
    <section>
      <h3 class="text-sm font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-4">Email Accounts</h3>
      <p class="text-sm text-gray-400 dark:text-gray-500">Manage connected accounts in a future version.</p>
    </section>
  </div>
</div>
