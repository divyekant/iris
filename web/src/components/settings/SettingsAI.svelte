<script lang="ts">
  import { api } from '../../lib/api';
  import FormInput from '../shared/FormInput.svelte';
  import FormToggle from '../shared/FormToggle.svelte';

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
  let decayThresholdDays = $state('7');
  let decaySaving = $state(false);

  // Writing style
  let styleTraits = $state<{ trait_type: string; trait_value: string; confidence: number; examples: string[] | null }[]>([]);
  let styleLoading = $state(false);
  let styleAnalyzing = $state(false);
  let styleMessage = $state('');

  // Auto-draft
  let autoDraftEnabled = $state(false);
  let autoDraftSensitivity = $state('balanced');
  let autoDraftSaving = $state(false);
  let autoDraftInitialized = $state(false);

  // Provider API keys (masked display)
  let anthropicKey = $state('');
  let anthropicModel = $state('');
  let openaiKey = $state('');
  let openaiModel = $state('');

  // Track initialization to avoid firing effects on load
  let aiInitialized = $state(false);
  let decayInitialized = $state(false);

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

  // Watch aiEnabled toggle changes (after initialization)
  $effect(() => {
    const val = aiEnabled;
    if (aiInitialized) {
      saveAiConfig();
    }
  });

  async function saveDecayConfig() {
    decaySaving = true;
    try {
      const result = await api.ai.setConfig({
        decay_enabled: decayEnabled,
        decay_threshold_days: parseInt(decayThresholdDays),
      });
      decayEnabled = result.decay_enabled;
      decayThresholdDays = String(result.decay_threshold_days);
    } catch {
      // Silently fail
    } finally {
      decaySaving = false;
    }
  }

  // Watch decayEnabled toggle changes (after initialization)
  $effect(() => {
    const val = decayEnabled;
    if (decayInitialized) {
      saveDecayConfig();
    }
  });

  async function loadWritingStyle() {
    styleLoading = true;
    try {
      const accounts = await api.accounts.list();
      if (accounts.length > 0) {
        const result = await api.style.get(accounts[0].id);
        styleTraits = result.traits;
      }
    } catch {
      // Style not available
    } finally {
      styleLoading = false;
    }
  }

  async function analyzeWritingStyle() {
    styleAnalyzing = true;
    styleMessage = '';
    try {
      const accounts = await api.accounts.list();
      if (accounts.length === 0) {
        styleMessage = 'No accounts found';
        return;
      }
      const result = await api.style.analyze(accounts[0].id);
      styleTraits = result.traits;
      styleMessage = result.emails_analyzed > 0
        ? `Analyzed ${result.emails_analyzed} emails, found ${result.traits.length} traits`
        : 'No sent emails found to analyze';
    } catch {
      styleMessage = 'Analysis failed';
    } finally {
      styleAnalyzing = false;
      setTimeout(() => { styleMessage = ''; }, 5000);
    }
  }

  async function loadAutoDraftConfig() {
    try {
      const config = await api.autoDraft.getConfig();
      autoDraftEnabled = config.enabled;
      autoDraftSensitivity = config.sensitivity;
    } catch {
      // Not available
    }
    autoDraftInitialized = true;
  }

  async function saveAutoDraftConfig() {
    autoDraftSaving = true;
    try {
      const result = await api.autoDraft.setConfig({
        enabled: autoDraftEnabled,
        sensitivity: autoDraftSensitivity,
      });
      autoDraftEnabled = result.enabled;
      autoDraftSensitivity = result.sensitivity;
    } catch {
      // Silently fail
    } finally {
      autoDraftSaving = false;
    }
  }

  // Watch autoDraftEnabled toggle changes (after initialization)
  $effect(() => {
    const val = autoDraftEnabled;
    if (autoDraftInitialized) {
      saveAutoDraftConfig();
    }
  });

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
        decayThresholdDays = String(aiConfig.decay_threshold_days ?? 7);
      } catch {
        // AI config not available
      }
      // Mark as initialized after load so effects don't fire during setup
      aiInitialized = true;
      decayInitialized = true;
    }
    loadSettings();
    loadWritingStyle();
    loadAutoDraftConfig();
  });
</script>

<div class="space-y-8">
  <!-- AI section -->
  <section>
    <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">AI Processing</h3>
    <div class="space-y-4">
      <!-- Enable/disable toggle -->
      <FormToggle
        label="Enable AI classification"
        description="Classify and summarize emails using AI providers"
        bind:checked={aiEnabled}
      />

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
        <div class="space-y-2">
          <FormInput
            type="password"
            bind:value={anthropicKey}
            placeholder="Paste Anthropic API key or OAuth token"
          />
          <FormInput
            bind:value={anthropicModel}
            placeholder="Model (default: claude-haiku-4-5-20251001)"
          />
        </div>
      </div>

      <!-- OpenAI -->
      <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
        <p class="text-sm font-medium mb-0.5" style="color: var(--iris-color-text);">OpenAI</p>
        <p class="text-xs mb-2" style="color: var(--iris-color-text-faint);">API key (sk-...) or ChatGPT subscription token</p>
        <div class="space-y-2">
          <FormInput
            type="password"
            bind:value={openaiKey}
            placeholder="Paste OpenAI API key or ChatGPT token"
          />
          <FormInput
            bind:value={openaiModel}
            placeholder="Model (default: gpt-4o-mini)"
          />
        </div>
      </div>

      <!-- Ollama -->
      <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
        <p class="text-sm font-medium mb-0.5" style="color: var(--iris-color-text);">Ollama (local)</p>
        <p class="text-xs mb-2" style="color: var(--iris-color-text-faint);">Free local AI — requires Ollama running on your machine</p>
        <div class="flex gap-2">
          <div class="flex-1">
            <FormInput
              bind:value={aiOllamaUrl}
              placeholder="http://localhost:11434"
            />
          </div>
          <div class="w-40">
            <FormInput
              bind:value={aiModel}
              placeholder="e.g. llama3.2:3b"
            />
          </div>
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
        <div class="space-y-2">
          <FormInput
            bind:value={memoriesUrl}
            placeholder="http://localhost:8900"
          />
          <FormInput
            type="password"
            bind:value={memoriesKey}
            placeholder="API key (optional)"
          />
        </div>
        <div class="flex items-center gap-2 mt-2">
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
        <FormToggle
          label="Priority Decay"
          description="Automatically reduce priority of inactive threads over time"
          bind:checked={decayEnabled}
        />
        {#if decayEnabled}
          <div class="flex items-center gap-2 mt-3">
            <label for="decay-days" class="text-xs" style="color: var(--iris-color-text-muted);">Decay after</label>
            <div class="w-16">
              <FormInput
                name="decay-days"
                type="number"
                bind:value={decayThresholdDays}
              />
            </div>
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

  <!-- Writing Style section -->
  <section>
    <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Writing Style</h3>
    <div class="space-y-4">
      <p class="text-xs" style="color: var(--iris-color-text-faint);">
        Iris learns your writing style from sent emails to generate drafts that sound like you.
      </p>

      {#if styleLoading}
        <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
          <p class="text-sm" style="color: var(--iris-color-text-muted);">Loading style traits...</p>
        </div>
      {:else if styleTraits.length > 0}
        <div class="grid gap-2">
          {#each styleTraits as trait}
            <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
              <div class="flex items-center justify-between mb-1">
                <span class="text-xs font-medium uppercase tracking-wider" style="color: var(--iris-color-text-muted);">
                  {trait.trait_type === 'avg_length' ? 'Avg Length' : trait.trait_type === 'signoff' ? 'Sign-off' : trait.trait_type.charAt(0).toUpperCase() + trait.trait_type.slice(1)}
                </span>
                <span class="text-[10px] px-1.5 py-0.5 rounded-full" style="background: color-mix(in srgb, var(--iris-color-primary) {Math.round(trait.confidence * 100)}%, transparent); color: var(--iris-color-primary);">
                  {Math.round(trait.confidence * 100)}%
                </span>
              </div>
              <p class="text-sm" style="color: var(--iris-color-text);">{trait.trait_value}</p>
              {#if trait.examples && trait.examples.length > 0}
                <div class="mt-1 flex flex-wrap gap-1">
                  {#each trait.examples.slice(0, 3) as example}
                    <span class="text-[10px] px-1.5 py-0.5 rounded" style="background: var(--iris-color-bg-surface); color: var(--iris-color-text-faint);">
                      "{example}"
                    </span>
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {:else}
        <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
          <p class="text-sm" style="color: var(--iris-color-text-muted);">No writing style detected yet. Click "Analyze" to learn from your sent emails.</p>
        </div>
      {/if}

      <div class="flex items-center gap-2">
        <button
          class="settings-btn-primary px-4 py-2 text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
          style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
          onclick={analyzeWritingStyle}
          disabled={styleAnalyzing}
        >
          {styleAnalyzing ? 'Analyzing...' : styleTraits.length > 0 ? 'Re-analyze' : 'Analyze Writing Style'}
        </button>
        {#if styleMessage}
          <span class="text-xs" style="color: var(--iris-color-text-muted);">{styleMessage}</span>
        {/if}
      </div>
    </div>
  </section>

  <!-- Auto-Draft section -->
  <section>
    <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Auto-Draft</h3>
    <div class="space-y-4">
      <FormToggle
        label="Auto-generate reply drafts"
        description="Automatically draft replies for incoming emails using AI and your writing style"
        bind:checked={autoDraftEnabled}
      />

      {#if autoDraftEnabled}
        <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
          <p class="text-xs font-medium mb-2" style="color: var(--iris-color-text-muted);">Sensitivity</p>
          <div class="flex gap-2">
            {#each ['conservative', 'balanced', 'aggressive'] as level}
              <button
                class="flex-1 px-3 py-2 text-xs font-medium rounded-lg border transition-colors"
                style="border-color: {autoDraftSensitivity === level ? 'var(--iris-color-primary)' : 'var(--iris-color-border)'}; background: {autoDraftSensitivity === level ? 'color-mix(in srgb, var(--iris-color-primary) 12%, transparent)' : 'transparent'}; color: {autoDraftSensitivity === level ? 'var(--iris-color-primary)' : 'var(--iris-color-text-muted)'};"
                onclick={() => { autoDraftSensitivity = level; saveAutoDraftConfig(); }}
              >
                {level.charAt(0).toUpperCase() + level.slice(1)}
              </button>
            {/each}
          </div>
          <p class="text-[10px] mt-2" style="color: var(--iris-color-text-faint);">
            {autoDraftSensitivity === 'conservative' ? 'Only draft replies for well-matched patterns (high confidence required)' :
             autoDraftSensitivity === 'aggressive' ? 'Draft replies for most incoming emails (lower confidence threshold)' :
             'Draft replies when reasonably confident about the appropriate response'}
          </p>
        </div>
      {/if}
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
</style>
