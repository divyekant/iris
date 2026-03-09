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
  let aiProviders = $state<{ name: string; model: string; healthy: boolean }[]>([]);
  let aiTesting = $state(false);
  let aiSaving = $state(false);
  let memoriesUrl = $state('');
  let memoriesKey = $state('');
  let memoriesConnected = $state(false);
  let memoriesSaving = $state(false);
  let memoriesTesting = $state(false);
  let memoriesStatus = $state<string | null>(null);

  // AI reprocess
  let reprocessing = $state(false);
  let reprocessMessage = $state('');

  // Fix encoding
  let fixingEncoding = $state(false);
  let fixEncodingMessage = $state('');

  // Provider API keys (masked display)
  let anthropicKey = $state('');
  let anthropicModel = $state('');
  let openaiKey = $state('');
  let openaiModel = $state('');

  // Signatures
  type SignatureItem = { id: string; account_id: string; name: string; body_text: string; body_html: string; is_default: boolean; created_at: number };
  let accounts = $state<any[]>([]);
  let sigAccountId = $state('');
  let sigList = $state<SignatureItem[]>([]);
  let sigEditing = $state<string | null>(null);
  let sigEditName = $state('');
  let sigEditText = $state('');
  let sigEditDefault = $state(false);
  let sigNewName = $state('');
  let sigNewText = $state('');
  let sigNewDefault = $state(false);
  let sigSaving = $state(false);
  let sigShowNew = $state(false);

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

  async function testAiConnection() {
    aiTesting = true;
    try {
      const result = await api.ai.testConnection();
      aiProviders = result.providers;
      aiConnected = result.providers.some(p => p.healthy);
    } catch {
      aiConnected = false;
      aiProviders = [];
    } finally {
      aiTesting = false;
    }
  }

  async function saveAiConfig() {
    aiSaving = true;
    try {
      const data: any = {
        ollama_url: aiOllamaUrl,
        model: aiModel,
        enabled: aiEnabled,
      };
      // Only send keys if they've been changed (non-empty = user typed something)
      if (anthropicKey) data.anthropic_api_key = anthropicKey;
      if (anthropicModel) data.anthropic_model = anthropicModel;
      if (openaiKey) data.openai_api_key = openaiKey;
      if (openaiModel) data.openai_model = openaiModel;

      const result = await api.ai.setConfig(data);
      aiConnected = result.connected;
      aiProviders = result.providers;
      // Clear key fields after save (stored server-side)
      anthropicKey = '';
      openaiKey = '';
    } catch {
      // Silently fail
    } finally {
      aiSaving = false;
    }
  }

  async function saveMemoriesConfig() {
    memoriesSaving = true;
    memoriesStatus = null;
    try {
      const data: any = { memories_url: memoriesUrl };
      if (memoriesKey) data.memories_api_key = memoriesKey;
      const result = await api.ai.setConfig(data);
      memoriesUrl = result.memories_url || memoriesUrl;
      memoriesConnected = result.memories_connected;
      memoriesKey = '';
      memoriesStatus = memoriesConnected ? 'Saved & connected' : 'Saved (not reachable)';
    } catch {
      memoriesStatus = 'Save failed';
    } finally {
      memoriesSaving = false;
      setTimeout(() => { memoriesStatus = null; }, 3000);
    }
  }

  async function testMemoriesConnection() {
    memoriesTesting = true;
    memoriesStatus = null;
    try {
      const result = await api.ai.getConfig();
      memoriesConnected = result.memories_connected;
      memoriesStatus = memoriesConnected ? 'Connected' : 'Not reachable';
    } catch {
      memoriesConnected = false;
      memoriesStatus = 'Test failed';
    } finally {
      memoriesTesting = false;
      setTimeout(() => { memoriesStatus = null; }, 3000);
    }
  }

  async function toggleAi() {
    aiEnabled = !aiEnabled;
    await saveAiConfig();
  }

  async function reprocessUntagged() {
    reprocessing = true;
    reprocessMessage = '';
    try {
      const result = await api.ai.reprocess();
      reprocessMessage = result.enqueued > 0
        ? `Enqueued ${result.enqueued} message${result.enqueued === 1 ? '' : 's'} for AI classification.`
        : 'All messages are already tagged.';
    } catch {
      reprocessMessage = 'Failed to trigger reprocessing.';
    } finally {
      reprocessing = false;
    }
  }

  async function fixEncoding() {
    fixingEncoding = true;
    fixEncodingMessage = '';
    try {
      const result = await api.messages.fixEncoding();
      fixEncodingMessage = result.fixed > 0
        ? `Fixed ${result.fixed} message${result.fixed === 1 ? '' : 's'} with encoded subjects.`
        : 'No encoded subjects found.';
    } catch {
      fixEncodingMessage = 'Failed to fix encoding.';
    } finally {
      fixingEncoding = false;
    }
  }

  async function loadAccounts() {
    try {
      accounts = await api.accounts.list();
      if (accounts.length > 0 && !sigAccountId) {
        sigAccountId = accounts[0].id;
        await loadSignatures();
      }
    } catch { accounts = []; }
  }

  async function loadSignatures() {
    if (!sigAccountId) { sigList = []; return; }
    try {
      sigList = await api.signatures.list(sigAccountId);
    } catch { sigList = []; }
  }

  async function createSignature() {
    if (!sigNewName.trim() || !sigAccountId || sigSaving) return;
    sigSaving = true;
    try {
      await api.signatures.create({
        account_id: sigAccountId,
        name: sigNewName.trim(),
        body_text: sigNewText,
        is_default: sigNewDefault,
      });
      sigNewName = '';
      sigNewText = '';
      sigNewDefault = false;
      sigShowNew = false;
      await loadSignatures();
    } catch { /* silently fail */ }
    finally { sigSaving = false; }
  }

  function startEditSignature(sig: SignatureItem) {
    sigEditing = sig.id;
    sigEditName = sig.name;
    sigEditText = sig.body_text;
    sigEditDefault = sig.is_default;
  }

  function cancelEditSignature() {
    sigEditing = null;
    sigEditName = '';
    sigEditText = '';
    sigEditDefault = false;
  }

  async function saveEditSignature() {
    if (!sigEditing || sigSaving) return;
    sigSaving = true;
    try {
      await api.signatures.update(sigEditing, {
        name: sigEditName.trim(),
        body_text: sigEditText,
        is_default: sigEditDefault,
      });
      sigEditing = null;
      await loadSignatures();
    } catch { /* silently fail */ }
    finally { sigSaving = false; }
  }

  async function deleteSignature(id: string) {
    try {
      await api.signatures.delete(id);
      if (sigEditing === id) sigEditing = null;
      await loadSignatures();
    } catch { /* silently fail */ }
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
        aiProviders = aiConfig.providers || [];
        memoriesUrl = aiConfig.memories_url || '';
        memoriesConnected = aiConfig.memories_connected;
        memoriesKey = '';
      } catch {
        // AI config not available
      }

      await loadAccounts();
      await loadApiKeys();
      await loadAuditLog();

      loading = false;
    }
    loadSettings();
  });
</script>

<div class="max-w-2xl mx-auto py-12 px-6">
  <h2 class="text-2xl font-bold mb-8" style="color: var(--iris-color-text);">Settings</h2>

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
    </section>

    <!-- AI section -->
    <section>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">AI Processing</h3>
      <div class="space-y-4">
        <!-- Enable/disable toggle -->
        <div class="flex items-center justify-between">
          <div>
            <p class="text-sm font-medium" style="color: var(--iris-color-text);">Enable AI classification</p>
            <p class="text-xs" style="color: var(--iris-color-text-faint);">Classify and summarize emails using AI providers</p>
          </div>
          <button
            class="relative w-11 h-6 rounded-full transition-colors"
            style="background: {aiEnabled ? 'var(--iris-color-primary)' : 'var(--iris-color-border-subtle)'};"
            onclick={toggleAi}
          >
            <span class="absolute top-0.5 left-0.5 w-5 h-5 rounded-full shadow transition-transform {aiEnabled ? 'translate-x-5' : ''}" style="background: var(--iris-color-text);"></span>
          </button>
        </div>

        <!-- Provider Status -->
        {#if aiProviders.length > 0}
          <div class="flex flex-wrap gap-2">
            {#each aiProviders as provider}
              <div class="flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs" style="background: var(--iris-color-bg-surface); border: 1px solid var(--iris-color-border);">
                <div class="w-2 h-2 rounded-full" style="background: {provider.healthy ? 'var(--iris-color-success)' : 'var(--iris-color-error)'};"></div>
                <span class="font-medium" style="color: var(--iris-color-text);">{provider.name}</span>
                {#if provider.model}
                  <span style="color: var(--iris-color-text-faint);">{provider.model}</span>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        <!-- Anthropic -->
        <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
          <p class="text-sm font-medium mb-0.5" style="color: var(--iris-color-text);">Anthropic</p>
          <p class="text-xs mb-2" style="color: var(--iris-color-text-faint);">API key (sk-ant-api03-...) or OAuth token (sk-ant-oat01-...)</p>
          <input
            type="password"
            bind:value={anthropicKey}
            placeholder="Paste Anthropic API key or OAuth token"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm mb-2 focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
          />
          <input
            type="text"
            bind:value={anthropicModel}
            placeholder="Model (default: claude-haiku-4-5-20251001)"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
          />
        </div>

        <!-- OpenAI -->
        <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
          <p class="text-sm font-medium mb-0.5" style="color: var(--iris-color-text);">OpenAI</p>
          <p class="text-xs mb-2" style="color: var(--iris-color-text-faint);">API key (sk-...) or ChatGPT subscription token</p>
          <input
            type="password"
            bind:value={openaiKey}
            placeholder="Paste OpenAI API key or ChatGPT token"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm mb-2 focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
          />
          <input
            type="text"
            bind:value={openaiModel}
            placeholder="Model (default: gpt-4o-mini)"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
          />
        </div>

        <!-- Ollama -->
        <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
          <p class="text-sm font-medium mb-0.5" style="color: var(--iris-color-text);">Ollama (local)</p>
          <p class="text-xs mb-2" style="color: var(--iris-color-text-faint);">Free local AI — requires Ollama running on your machine</p>
          <div class="flex gap-2">
            <input
              type="text"
              bind:value={aiOllamaUrl}
              placeholder="http://localhost:11434"
              class="settings-input flex-1 px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
              style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
            />
            <input
              type="text"
              bind:value={aiModel}
              placeholder="e.g. llama3.2:3b"
              class="settings-input w-40 px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
              style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
            />
          </div>
        </div>

        <!-- Save + Test buttons -->
        <div class="flex gap-2">
          <button
            class="settings-btn-primary px-4 py-2 text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
            style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
            onclick={saveAiConfig}
            disabled={aiSaving}
          >
            {aiSaving ? 'Saving...' : 'Save AI Settings'}
          </button>
          <button
            class="settings-btn-secondary px-4 py-2 text-sm rounded-lg border transition-colors disabled:opacity-50"
            style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
            onclick={testAiConnection}
            disabled={aiTesting}
          >
            {aiTesting ? 'Testing...' : 'Test Providers'}
          </button>
        </div>

        <!-- Reprocess untagged -->
        <div class="flex items-center gap-3">
          <button
            class="settings-btn-secondary px-4 py-2 text-sm rounded-lg border transition-colors disabled:opacity-50"
            style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
            onclick={reprocessUntagged}
            disabled={reprocessing}
          >
            {reprocessing ? 'Processing...' : 'Reprocess untagged'}
          </button>
          {#if reprocessMessage}
            <span class="text-xs" style="color: var(--iris-color-text-muted);">{reprocessMessage}</span>
          {/if}
        </div>

        <!-- Fix encoded subjects -->
        <div class="flex items-center gap-3">
          <button
            class="settings-btn-secondary px-4 py-2 text-sm rounded-lg border transition-colors disabled:opacity-50"
            style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
            onclick={fixEncoding}
            disabled={fixingEncoding}
          >
            {fixingEncoding ? 'Fixing...' : 'Fix encoded subjects'}
          </button>
          {#if fixEncodingMessage}
            <span class="text-xs" style="color: var(--iris-color-text-muted);">{fixEncodingMessage}</span>
          {/if}
        </div>

        <!-- Memories (Semantic Search) -->
        <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
          <div class="flex items-center justify-between mb-0.5">
            <p class="text-sm font-medium" style="color: var(--iris-color-text);">Semantic Search (Memories)</p>
            <div class="flex items-center gap-1.5">
              <div class="w-2 h-2 rounded-full" style="background: {memoriesConnected ? 'var(--iris-color-success)' : 'var(--iris-color-error)'};"></div>
              <span class="text-xs" style="color: var(--iris-color-text-faint);">
                {memoriesConnected ? 'Connected' : 'Not connected'}
              </span>
            </div>
          </div>
          <p class="text-xs mb-2" style="color: var(--iris-color-text-faint);">Vector-based search for meaning, not just keywords</p>
          <input
            type="text"
            bind:value={memoriesUrl}
            placeholder="http://localhost:8900"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm mb-2 focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
          />
          <input
            type="password"
            bind:value={memoriesKey}
            placeholder="API key (optional)"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm mb-2 focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
          />
          <div class="flex items-center gap-2">
            <button
              class="settings-btn-primary px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-50"
              style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
              onclick={saveMemoriesConfig}
              disabled={memoriesSaving}
            >
              {memoriesSaving ? 'Saving...' : 'Save'}
            </button>
            <button
              class="settings-btn-secondary px-3 py-1.5 text-xs rounded-lg border transition-colors disabled:opacity-50"
              style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
              onclick={testMemoriesConnection}
              disabled={memoriesTesting}
            >
              {memoriesTesting ? 'Testing...' : 'Test Connection'}
            </button>
            {#if memoriesStatus}
              <span class="text-xs" style="color: {memoriesStatus.includes('fail') || memoriesStatus.includes('not') ? 'var(--iris-color-error)' : 'var(--iris-color-success)'};">{memoriesStatus}</span>
            {/if}
          </div>
        </div>
      </div>
    </section>

    <!-- Signatures section -->
    <section>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Signatures</h3>
      <p class="text-xs mb-4" style="color: var(--iris-color-text-faint);">Create email signatures per account. The default signature auto-appends when composing.</p>

      <!-- Account picker -->
      {#if accounts.length > 1}
        <div class="mb-4">
          <select
            bind:value={sigAccountId}
            onchange={() => loadSignatures()}
            class="settings-input px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
          >
            {#each accounts as acct}
              <option value={acct.id}>{acct.email}</option>
            {/each}
          </select>
        </div>
      {:else if accounts.length === 1}
        <p class="text-xs mb-4" style="color: var(--iris-color-text-muted);">Account: {accounts[0].email}</p>
      {/if}

      <!-- Existing signatures -->
      {#if sigList.length > 0}
        <div class="space-y-3 mb-4">
          {#each sigList as sig}
            <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
              {#if sigEditing === sig.id}
                <!-- Edit mode -->
                <div class="space-y-2">
                  <input
                    type="text"
                    bind:value={sigEditName}
                    placeholder="Signature name"
                    class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
                    style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
                  />
                  <textarea
                    bind:value={sigEditText}
                    rows="4"
                    placeholder="Signature text"
                    class="settings-input w-full px-3 py-2 rounded-lg border text-sm resize-y focus:outline-none focus:ring-2"
                    style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
                  ></textarea>
                  <div class="flex items-center gap-3">
                    <label class="flex items-center gap-2 text-sm cursor-pointer" style="color: var(--iris-color-text);">
                      <input type="checkbox" bind:checked={sigEditDefault} class="rounded" />
                      Default
                    </label>
                    <span class="flex-1"></span>
                    <button
                      class="settings-btn-secondary px-3 py-1.5 text-xs rounded-lg border transition-colors"
                      style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
                      onclick={cancelEditSignature}
                    >Cancel</button>
                    <button
                      class="settings-btn-primary px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-50"
                      style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
                      onclick={saveEditSignature}
                      disabled={sigSaving || !sigEditName.trim()}
                    >{sigSaving ? 'Saving...' : 'Save'}</button>
                  </div>
                </div>
              {:else}
                <!-- Display mode -->
                <div class="flex items-start justify-between gap-3">
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2 mb-1">
                      <span class="text-sm font-medium" style="color: var(--iris-color-text);">{sig.name}</span>
                      {#if sig.is_default}
                        <span class="px-1.5 py-0.5 text-[10px] rounded-full font-medium" style="background: color-mix(in srgb, var(--iris-color-primary) 20%, transparent); color: var(--iris-color-primary);">default</span>
                      {/if}
                    </div>
                    <pre class="text-xs whitespace-pre-wrap" style="color: var(--iris-color-text-faint); font-family: inherit;">{sig.body_text || '(empty)'}</pre>
                  </div>
                  <div class="flex items-center gap-1 shrink-0">
                    <button
                      class="settings-btn-secondary px-2 py-1 text-xs rounded border transition-colors"
                      style="border-color: var(--iris-color-border); color: var(--iris-color-text-muted);"
                      onclick={() => startEditSignature(sig)}
                    >Edit</button>
                    <button
                      class="settings-revoke-btn px-2 py-1 text-xs rounded transition-colors"
                      style="color: var(--iris-color-error);"
                      onclick={() => deleteSignature(sig.id)}
                    >Delete</button>
                  </div>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {:else if sigAccountId}
        <p class="text-sm mb-4" style="color: var(--iris-color-text-faint);">No signatures yet for this account.</p>
      {/if}

      <!-- Add new signature -->
      {#if sigShowNew}
        <div class="p-3 rounded-lg border space-y-2" style="border-color: var(--iris-color-border);">
          <input
            type="text"
            bind:value={sigNewName}
            placeholder="Signature name (e.g., Work, Personal)"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
          />
          <textarea
            bind:value={sigNewText}
            rows="4"
            placeholder="Best regards,&#10;Your Name&#10;your@email.com"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm resize-y focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
          ></textarea>
          <div class="flex items-center gap-3">
            <label class="flex items-center gap-2 text-sm cursor-pointer" style="color: var(--iris-color-text);">
              <input type="checkbox" bind:checked={sigNewDefault} class="rounded" />
              Set as default
            </label>
            <span class="flex-1"></span>
            <button
              class="settings-btn-secondary px-3 py-1.5 text-xs rounded-lg border transition-colors"
              style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
              onclick={() => { sigShowNew = false; sigNewName = ''; sigNewText = ''; sigNewDefault = false; }}
            >Cancel</button>
            <button
              class="settings-btn-primary px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-50"
              style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
              onclick={createSignature}
              disabled={sigSaving || !sigNewName.trim()}
            >{sigSaving ? 'Creating...' : 'Create'}</button>
          </div>
        </div>
      {:else}
        <button
          class="settings-btn-secondary px-4 py-2 text-sm rounded-lg border transition-colors disabled:opacity-50"
          style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
          onclick={() => (sigShowNew = true)}
          disabled={!sigAccountId}
        >
          + Add Signature
        </button>
      {/if}
    </section>

    <!-- API Keys section -->
    <section>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">API Keys</h3>
      <p class="text-xs mb-4" style="color: var(--iris-color-text-faint);">Create API keys for external agents to access your inbox.</p>

      <div class="flex gap-2 mb-4">
        <input
          type="text"
          bind:value={newKeyName}
          placeholder="Key name (e.g., Claude agent)"
          class="settings-input flex-1 px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
          style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
        />
        <select
          bind:value={newKeyPermission}
          class="settings-input px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
          style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
        >
          <option value="read_only">Read Only</option>
          <option value="draft_only">Draft Only</option>
          <option value="send_with_approval">Send w/ Approval</option>
          <option value="autonomous">Autonomous</option>
        </select>
        <button
          class="settings-btn-primary px-4 py-2 text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
          style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
          onclick={createApiKey}
          disabled={keyCreating || !newKeyName.trim()}
        >
          {keyCreating ? 'Creating...' : 'Create'}
        </button>
      </div>

      {#if createdKey}
        <div class="mb-4 p-3 rounded-lg border" style="background: color-mix(in srgb, var(--iris-color-success) 10%, transparent); border-color: color-mix(in srgb, var(--iris-color-success) 30%, transparent);">
          <p class="text-sm font-medium mb-1" style="color: var(--iris-color-success);">API key created! Copy it now — it won't be shown again.</p>
          <code class="block p-2 rounded text-xs select-all break-all" style="background: var(--iris-color-bg-surface); color: var(--iris-color-text); font-family: var(--iris-font-mono);">{createdKey}</code>
        </div>
      {/if}

      {#if apiKeys.length > 0}
        <div class="border rounded-lg overflow-hidden" style="border-color: var(--iris-color-border);">
          <table class="w-full text-sm">
            <thead style="background: var(--iris-color-bg-surface);">
              <tr>
                <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Name</th>
                <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Permission</th>
                <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Last Used</th>
                <th class="text-right px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);"></th>
              </tr>
            </thead>
            <tbody>
              {#each apiKeys as key}
                <tr class="border-t" style="border-color: var(--iris-color-border-subtle);">
                  <td class="px-3 py-2" style="color: var(--iris-color-text);">
                    <span class="font-medium">{key.name}</span>
                    <span class="text-xs ml-1" style="color: var(--iris-color-text-faint);">{key.key_prefix}...</span>
                  </td>
                  <td class="px-3 py-2">
                    <span class="px-2 py-0.5 text-xs rounded-full" style="background: var(--iris-color-bg-surface); color: var(--iris-color-text-muted);">
                      {key.permission.replace(/_/g, ' ')}
                    </span>
                  </td>
                  <td class="px-3 py-2 text-xs" style="color: var(--iris-color-text-faint);">
                    {key.last_used_at ? formatTimestamp(key.last_used_at) : 'Never'}
                  </td>
                  <td class="px-3 py-2 text-right">
                    <button
                      class="settings-revoke-btn text-xs"
                      style="color: var(--iris-color-error);"
                      onclick={() => revokeApiKey(key.id)}
                    >Revoke</button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p class="text-sm" style="color: var(--iris-color-text-faint);">No API keys created yet.</p>
      {/if}
    </section>

    <!-- Audit Log section -->
    <section>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Audit Log</h3>
      <p class="text-xs mb-4" style="color: var(--iris-color-text-faint);">Recent agent activity.</p>

      {#if auditEntries.length > 0}
        <div class="border rounded-lg overflow-hidden" style="border-color: var(--iris-color-border);">
          <table class="w-full text-sm">
            <thead style="background: var(--iris-color-bg-surface);">
              <tr>
                <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Time</th>
                <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Agent</th>
                <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Action</th>
                <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Status</th>
              </tr>
            </thead>
            <tbody>
              {#each auditEntries as entry}
                <tr class="border-t" style="border-color: var(--iris-color-border-subtle);">
                  <td class="px-3 py-2 text-xs" style="color: var(--iris-color-text-faint);">{formatTimestamp(entry.created_at)}</td>
                  <td class="px-3 py-2 text-xs" style="color: var(--iris-color-text);">{entry.key_name || entry.api_key_id?.slice(0, 8) || '—'}</td>
                  <td class="px-3 py-2 text-xs" style="color: var(--iris-color-text);">
                    {entry.action}
                    {#if entry.resource_type}
                      <span style="color: var(--iris-color-text-faint);"> on {entry.resource_type}</span>
                    {/if}
                  </td>
                  <td class="px-3 py-2 text-xs">
                    <span class="px-1.5 py-0.5 rounded text-[10px] font-medium"
                      style="background: {entry.status === 'success' ? 'color-mix(in srgb, var(--iris-color-success) 15%, transparent)'
                       : entry.status === 'denied' ? 'color-mix(in srgb, var(--iris-color-error) 15%, transparent)'
                       : 'var(--iris-color-bg-surface)'}; color: {entry.status === 'success' ? 'var(--iris-color-success)'
                       : entry.status === 'denied' ? 'var(--iris-color-error)'
                       : 'var(--iris-color-text-muted)'};">
                      {entry.status}
                    </span>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p class="text-sm" style="color: var(--iris-color-text-faint);">No agent activity yet.</p>
      {/if}
    </section>

    <!-- Accounts section -->
    <section>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Email Accounts</h3>
      <p class="text-sm" style="color: var(--iris-color-text-faint);">Manage connected accounts in a future version.</p>
    </section>
  </div>
</div>

<style>
  .settings-theme-card:hover {
    border-color: var(--iris-color-primary) !important;
  }

  .settings-btn-primary:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .settings-btn-secondary:hover:not(:disabled) {
    background: var(--iris-color-bg-surface);
  }

  .settings-revoke-btn:hover {
    filter: brightness(1.3);
  }

  .settings-input::placeholder {
    color: var(--iris-color-text-faint);
  }
</style>
