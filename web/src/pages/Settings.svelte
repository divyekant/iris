<script lang="ts">
  import { api } from '../lib/api';

  type Theme = 'light' | 'dark' | 'system';

  let currentTheme = $state<Theme>('system');
  let loading = $state(true);

  // AI settings
  let aiOllamaUrl = $state('');
  let aiModel = $state('');
  let aiEnabled = $state(false);
  let aiConnected = $state(false);
  let aiModels = $state<string[]>([]);
  let aiTesting = $state(false);
  let aiSaving = $state(false);
  let memoriesUrl = $state('');
  let memoriesConnected = $state(false);

  // API key management
  let apiKeys = $state<any[]>([]);
  let newKeyName = $state('');
  let newKeyPermission = $state('read_only');
  let createdKey = $state('');
  let keyCreating = $state(false);

  // Audit log
  let auditEntries = $state<any[]>([]);

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

  async function testAiConnection() {
    aiTesting = true;
    try {
      const result = await api.ai.testConnection();
      aiConnected = result.connected;
      aiModels = result.models;
    } catch {
      aiConnected = false;
      aiModels = [];
    } finally {
      aiTesting = false;
    }
  }

  async function saveAiConfig() {
    aiSaving = true;
    try {
      const result = await api.ai.setConfig({
        ollama_url: aiOllamaUrl,
        model: aiModel,
        enabled: aiEnabled,
      });
      aiConnected = result.connected;
    } catch {
      // Silently fail
    } finally {
      aiSaving = false;
    }
  }

  async function toggleAi() {
    aiEnabled = !aiEnabled;
    await saveAiConfig();
  }

  async function loadApiKeys() {
    try {
      apiKeys = await api.apiKeys.list();
    } catch { apiKeys = []; }
  }

  async function createApiKey() {
    if (!newKeyName.trim() || keyCreating) return;
    keyCreating = true;
    createdKey = '';
    try {
      const result = await api.apiKeys.create({ name: newKeyName.trim(), permission: newKeyPermission });
      createdKey = result.key;
      newKeyName = '';
      await loadApiKeys();
    } catch { /* silently fail */ }
    finally { keyCreating = false; }
  }

  async function revokeApiKey(id: string) {
    try {
      await api.apiKeys.revoke(id);
      await loadApiKeys();
    } catch { /* silently fail */ }
  }

  async function loadAuditLog() {
    try {
      auditEntries = await api.auditLog.list({ limit: 25 });
    } catch { auditEntries = []; }
  }

  function formatTimestamp(ts: number): string {
    return new Date(ts * 1000).toLocaleString([], {
      month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit',
    });
  }

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
        const aiConfig = await api.ai.getConfig();
        aiOllamaUrl = aiConfig.ollama_url;
        aiModel = aiConfig.model;
        aiEnabled = aiConfig.enabled;
        aiConnected = aiConfig.connected;
        memoriesUrl = aiConfig.memories_url;
        memoriesConnected = aiConfig.memories_connected;
      } catch {
        // AI config not available
      }

      await loadApiKeys();
      await loadAuditLog();

      loading = false;
    }
    loadSettings();
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

    <!-- AI section -->
    <section>
      <h3 class="text-sm font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-4">AI Processing</h3>
      <div class="space-y-4">
        <!-- Enable/disable toggle -->
        <div class="flex items-center justify-between">
          <div>
            <p class="text-sm font-medium text-gray-700 dark:text-gray-300">Enable AI classification</p>
            <p class="text-xs text-gray-400 dark:text-gray-500">Classify emails on ingest using local Ollama</p>
          </div>
          <button
            class="relative w-11 h-6 rounded-full transition-colors {aiEnabled ? 'bg-blue-500' : 'bg-gray-300 dark:bg-gray-600'}"
            onclick={toggleAi}
          >
            <span class="absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white shadow transition-transform {aiEnabled ? 'translate-x-5' : ''}"></span>
          </button>
        </div>

        <!-- Ollama URL -->
        <div>
          <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">Ollama URL</label>
          <div class="flex gap-2">
            <input
              type="text"
              bind:value={aiOllamaUrl}
              placeholder="http://localhost:11434"
              class="flex-1 px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
            <button
              class="px-4 py-2 text-sm rounded-lg border border-gray-300 dark:border-gray-600 hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors disabled:opacity-50"
              onclick={testAiConnection}
              disabled={aiTesting}
            >
              {aiTesting ? 'Testing...' : 'Test'}
            </button>
          </div>
          <div class="mt-1 flex items-center gap-1.5">
            <div class="w-2 h-2 rounded-full {aiConnected ? 'bg-green-500' : 'bg-red-400'}"></div>
            <span class="text-xs text-gray-400 dark:text-gray-500">
              {aiConnected ? 'Connected' : 'Not connected'}
            </span>
          </div>
        </div>

        <!-- Model picker -->
        <div>
          <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">Model</label>
          {#if aiModels.length > 0}
            <select
              bind:value={aiModel}
              class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
              onchange={saveAiConfig}
            >
              <option value="">Select a model</option>
              {#each aiModels as model}
                <option value={model}>{model}</option>
              {/each}
            </select>
          {:else}
            <input
              type="text"
              bind:value={aiModel}
              placeholder="e.g. llama3.2:3b"
              class="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          {/if}
        </div>

        <!-- Save button -->
        <button
          class="px-4 py-2 text-sm font-medium rounded-lg bg-blue-500 text-white hover:bg-blue-600 transition-colors disabled:opacity-50"
          onclick={saveAiConfig}
          disabled={aiSaving}
        >
          {aiSaving ? 'Saving...' : 'Save AI Settings'}
        </button>

        <!-- Memories (Semantic Search) status -->
        <div class="pt-4 border-t border-gray-200 dark:border-gray-700">
          <p class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">Semantic Search (Memories)</p>
          <p class="text-xs text-gray-400 dark:text-gray-500 mb-2">Vector-based search for meaning, not just keywords</p>
          <div class="flex items-center gap-2">
            <code class="text-xs bg-gray-100 dark:bg-gray-800 px-2 py-1 rounded">{memoriesUrl}</code>
            <div class="flex items-center gap-1.5">
              <div class="w-2 h-2 rounded-full {memoriesConnected ? 'bg-green-500' : 'bg-red-400'}"></div>
              <span class="text-xs text-gray-400 dark:text-gray-500">
                {memoriesConnected ? 'Connected' : 'Not connected'}
              </span>
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- API Keys section -->
    <section>
      <h3 class="text-sm font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-4">API Keys</h3>
      <p class="text-xs text-gray-400 dark:text-gray-500 mb-4">Create API keys for external agents to access your inbox.</p>

      <div class="flex gap-2 mb-4">
        <input
          type="text"
          bind:value={newKeyName}
          placeholder="Key name (e.g., Claude agent)"
          class="flex-1 px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        />
        <select
          bind:value={newKeyPermission}
          class="px-3 py-2 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        >
          <option value="read_only">Read Only</option>
          <option value="draft_only">Draft Only</option>
          <option value="send_with_approval">Send w/ Approval</option>
          <option value="autonomous">Autonomous</option>
        </select>
        <button
          class="px-4 py-2 text-sm font-medium rounded-lg bg-blue-500 text-white hover:bg-blue-600 transition-colors disabled:opacity-50"
          onclick={createApiKey}
          disabled={keyCreating || !newKeyName.trim()}
        >
          {keyCreating ? 'Creating...' : 'Create'}
        </button>
      </div>

      {#if createdKey}
        <div class="mb-4 p-3 rounded-lg bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800">
          <p class="text-sm font-medium text-green-700 dark:text-green-400 mb-1">API key created! Copy it now — it won't be shown again.</p>
          <code class="block p-2 bg-white dark:bg-gray-800 rounded text-xs font-mono select-all break-all">{createdKey}</code>
        </div>
      {/if}

      {#if apiKeys.length > 0}
        <div class="border border-gray-200 dark:border-gray-700 rounded-lg overflow-hidden">
          <table class="w-full text-sm">
            <thead class="bg-gray-50 dark:bg-gray-800/50">
              <tr>
                <th class="text-left px-3 py-2 text-xs font-medium text-gray-500 dark:text-gray-400">Name</th>
                <th class="text-left px-3 py-2 text-xs font-medium text-gray-500 dark:text-gray-400">Permission</th>
                <th class="text-left px-3 py-2 text-xs font-medium text-gray-500 dark:text-gray-400">Last Used</th>
                <th class="text-right px-3 py-2 text-xs font-medium text-gray-500 dark:text-gray-400"></th>
              </tr>
            </thead>
            <tbody>
              {#each apiKeys as key}
                <tr class="border-t border-gray-100 dark:border-gray-800">
                  <td class="px-3 py-2">
                    <span class="font-medium">{key.name}</span>
                    <span class="text-xs text-gray-400 ml-1">{key.key_prefix}...</span>
                  </td>
                  <td class="px-3 py-2">
                    <span class="px-2 py-0.5 text-xs rounded-full bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400">
                      {key.permission.replace(/_/g, ' ')}
                    </span>
                  </td>
                  <td class="px-3 py-2 text-xs text-gray-400">
                    {key.last_used_at ? formatTimestamp(key.last_used_at) : 'Never'}
                  </td>
                  <td class="px-3 py-2 text-right">
                    <button
                      class="text-xs text-red-500 hover:text-red-700 dark:hover:text-red-400"
                      onclick={() => revokeApiKey(key.id)}
                    >Revoke</button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p class="text-sm text-gray-400 dark:text-gray-500">No API keys created yet.</p>
      {/if}
    </section>

    <!-- Audit Log section -->
    <section>
      <h3 class="text-sm font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-4">Audit Log</h3>
      <p class="text-xs text-gray-400 dark:text-gray-500 mb-4">Recent agent activity.</p>

      {#if auditEntries.length > 0}
        <div class="border border-gray-200 dark:border-gray-700 rounded-lg overflow-hidden">
          <table class="w-full text-sm">
            <thead class="bg-gray-50 dark:bg-gray-800/50">
              <tr>
                <th class="text-left px-3 py-2 text-xs font-medium text-gray-500 dark:text-gray-400">Time</th>
                <th class="text-left px-3 py-2 text-xs font-medium text-gray-500 dark:text-gray-400">Agent</th>
                <th class="text-left px-3 py-2 text-xs font-medium text-gray-500 dark:text-gray-400">Action</th>
                <th class="text-left px-3 py-2 text-xs font-medium text-gray-500 dark:text-gray-400">Status</th>
              </tr>
            </thead>
            <tbody>
              {#each auditEntries as entry}
                <tr class="border-t border-gray-100 dark:border-gray-800">
                  <td class="px-3 py-2 text-xs text-gray-400">{formatTimestamp(entry.created_at)}</td>
                  <td class="px-3 py-2 text-xs">{entry.key_name || entry.api_key_id?.slice(0, 8) || '—'}</td>
                  <td class="px-3 py-2 text-xs">
                    {entry.action}
                    {#if entry.resource_type}
                      <span class="text-gray-400"> on {entry.resource_type}</span>
                    {/if}
                  </td>
                  <td class="px-3 py-2 text-xs">
                    <span class="px-1.5 py-0.5 rounded text-[10px] font-medium
                      {entry.status === 'success' ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400'
                       : entry.status === 'denied' ? 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400'
                       : 'bg-gray-100 text-gray-600 dark:bg-gray-800 dark:text-gray-400'}">
                      {entry.status}
                    </span>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p class="text-sm text-gray-400 dark:text-gray-500">No agent activity yet.</p>
      {/if}
    </section>

    <!-- Accounts section -->
    <section>
      <h3 class="text-sm font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider mb-4">Email Accounts</h3>
      <p class="text-sm text-gray-400 dark:text-gray-500">Manage connected accounts in a future version.</p>
    </section>
  </div>
</div>
