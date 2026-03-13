<script lang="ts">
  import { api } from '../../lib/api';

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

  // Priority decay settings
  let decayEnabled = $state(true);
  let decayThresholdDays = $state(7);
  let decaySaving = $state(false);

  // Provider API keys (masked display)
  let anthropicKey = $state('');
  let anthropicModel = $state('');
  let openaiKey = $state('');
  let openaiModel = $state('');

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

  async function saveDecayConfig() {
    decaySaving = true;
    try {
      const result = await api.ai.setConfig({
        decay_enabled: decayEnabled,
        decay_threshold_days: decayThresholdDays,
      });
      decayEnabled = result.decay_enabled;
      decayThresholdDays = result.decay_threshold_days;
    } catch {
      // Silently fail
    } finally {
      decaySaving = false;
    }
  }

  async function toggleDecay() {
    decayEnabled = !decayEnabled;
    await saveDecayConfig();
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

  // Load settings on mount
  $effect(() => {
    async function loadSettings() {
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
        decayEnabled = aiConfig.decay_enabled ?? true;
        decayThresholdDays = aiConfig.decay_threshold_days ?? 7;
      } catch {
        // AI config not available
      }
    }
    loadSettings();
  });
</script>

<div class="space-y-8">
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
          aria-label="Toggle AI classification"
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

      <!-- Priority Decay -->
      <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
        <div class="flex items-center justify-between mb-0.5">
          <p class="text-sm font-medium" style="color: var(--iris-color-text);">Priority Decay</p>
          <button
            class="relative w-11 h-6 rounded-full transition-colors"
            style="background: {decayEnabled ? 'var(--iris-color-primary)' : 'var(--iris-color-border-subtle)'};"
            onclick={toggleDecay}
            aria-label="Toggle priority decay"
          >
            <span class="absolute top-0.5 left-0.5 w-5 h-5 rounded-full shadow transition-transform {decayEnabled ? 'translate-x-5' : ''}" style="background: var(--iris-color-text);"></span>
          </button>
        </div>
        <p class="text-xs mb-3" style="color: var(--iris-color-text-faint);">Automatically reduce priority of inactive threads over time</p>
        {#if decayEnabled}
          <div class="flex items-center gap-2">
            <label for="decay-days" class="text-xs" style="color: var(--iris-color-text-muted);">Decay after</label>
            <input
              id="decay-days"
              type="number"
              min="1"
              max="90"
              bind:value={decayThresholdDays}
              class="settings-input w-16 px-2 py-1.5 rounded-lg border text-sm text-center focus:outline-none focus:ring-2"
              style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
            />
            <span class="text-xs" style="color: var(--iris-color-text-muted);">days of inactivity</span>
            <button
              class="settings-btn-primary ml-auto px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-50"
              style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
              onclick={saveDecayConfig}
              disabled={decaySaving}
            >
              {decaySaving ? 'Saving...' : 'Save'}
            </button>
          </div>
        {/if}
      </div>
    </div>
  </section>
</div>

<style>
  .settings-btn-primary:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .settings-btn-secondary:hover:not(:disabled) {
    background: var(--iris-color-bg-surface);
  }

  .settings-input::placeholder {
    color: var(--iris-color-text-faint);
  }
</style>
