<script lang="ts">
  import { api } from '../lib/api';
  import { requestNotificationPermission, setEnabled as setNotificationsEnabled, getPermissionState } from '../lib/notifications';
  import { Bell, BellOff } from 'lucide-svelte';

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

  // Undo send
  let undoSendDelay = $state(10);
  let undoSendSaving = $state(false);

  // Priority decay settings
  let decayEnabled = $state(true);
  let decayThresholdDays = $state(7);
  let decaySaving = $state(false);

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

  // Templates
  let templates = $state<any[]>([]);
  let editingTemplateId = $state<string | null>(null);
  let templateName = $state('');
  let templateSubject = $state('');
  let templateBody = $state('');
  let templateSaving = $state(false);

  async function loadTemplates() {
    try {
      templates = await api.templates.list();
    } catch { templates = []; }
  }

  function startNewTemplate() {
    editingTemplateId = 'new';
    templateName = '';
    templateSubject = '';
    templateBody = '';
  }

  function startEditTemplate(t: any) {
    editingTemplateId = t.id;
    templateName = t.name;
    templateSubject = t.subject || '';
    templateBody = t.body_text;
  }

  function cancelEditTemplate() {
    editingTemplateId = null;
    templateName = '';
    templateSubject = '';
    templateBody = '';
  }

  async function saveTemplate() {
    if (!templateName.trim() || !templateBody.trim()) return;
    templateSaving = true;
    try {
      const data = { name: templateName.trim(), subject: templateSubject.trim() || undefined, body_text: templateBody };
      if (editingTemplateId === 'new') {
        await api.templates.create(data);
      } else if (editingTemplateId) {
        await api.templates.update(editingTemplateId, { ...data, body_text: templateBody });
      }
      cancelEditTemplate();
      await loadTemplates();
    } catch { /* silently fail */ }
    finally { templateSaving = false; }
  }

  async function deleteTemplate(id: string) {
    try {
      await api.templates.delete(id);
      await loadTemplates();
    } catch { /* silently fail */ }
  }

  // Blocked senders
  let blockedSenders = $state<{ id: string; email_address: string; reason?: string; created_at: number }[]>([]);
  let newBlockEmail = $state('');
  let blockingEmail = $state(false);

  // API key management
  let apiKeys = $state<any[]>([]);
  let newKeyName = $state('');
  let newKeyPermission = $state('read_only');
  let createdKey = $state('');
  let keyCreating = $state(false);

  // Desktop notifications
  let notificationsEnabled = $state(localStorage.getItem('iris-notifications') !== 'false');
  let notificationPermission = $state(getPermissionState());

  // Per-account notification control
  let acctNotificationState = $state<Record<string, boolean>>({});
  let acctNotificationToggling = $state<Record<string, boolean>>({});

  // Labels
  type LabelItem = { id: string; name: string; color: string; created_at: number; message_count: number };
  let labelList2 = $state<LabelItem[]>([]);
  let labelShowNew = $state(false);
  let labelNewName = $state('');
  let labelNewColor = $state('#3B82F6');
  let labelSaving = $state(false);
  let labelEditing = $state<string | null>(null);
  let labelEditName = $state('');
  let labelEditColor = $state('');

  const LABEL_COLORS = ['#3B82F6', '#16A34A', '#EF4444', '#d4af37', '#8B5CF6', '#EC4899', '#06B6D4', '#F97316'];

  // Filter rules
  type FilterCondition = { field: string; operator: string; value: string };
  type FilterAction = { type: string; value?: string };
  type FilterRuleItem = { id: string; name: string; conditions: FilterCondition[]; actions: FilterAction[]; is_active: boolean; account_id: string | null; created_at: number };
  let filterRules = $state<FilterRuleItem[]>([]);
  let frShowNew = $state(false);
  let frNewName = $state('');
  let frNewConditions = $state<FilterCondition[]>([{ field: 'from', operator: 'contains', value: '' }]);
  let frNewActions = $state<FilterAction[]>([{ type: 'archive' }]);
  let frSaving = $state(false);
  let frEditing = $state<string | null>(null);
  let frEditName = $state('');
  let frEditConditions = $state<FilterCondition[]>([]);
  let frEditActions = $state<FilterAction[]>([]);
  let frEditActive = $state(true);

  // Aliases
  type AliasItem = { id: string; account_id: string; email: string; display_name: string; reply_to: string | null; is_default: boolean; created_at: number };
  let aliases = $state<AliasItem[]>([]);
  let aliasAccountId = $state('');
  let aliasShowNew = $state(false);
  let aliasNewEmail = $state('');
  let aliasNewDisplayName = $state('');
  let aliasNewReplyTo = $state('');
  let aliasNewDefault = $state(false);
  let aliasSaving = $state(false);
  let aliasEditing = $state<string | null>(null);
  let aliasEditEmail = $state('');
  let aliasEditDisplayName = $state('');
  let aliasEditReplyTo = $state('');
  let aliasEditDefault = $state(false);

  // Subscription audit
  type SubscriptionInfo = {
    sender: string;
    sender_name: string | null;
    total_count: number;
    read_count: number;
    read_rate: number;
    last_received: number;
    has_unsubscribe: boolean;
    category: string | null;
  };
  let subscriptions = $state<SubscriptionInfo[]>([]);
  let auditRunning = $state(false);
  let auditLoaded = $state(false);

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

  async function toggleNotifications() {
    if (!notificationsEnabled) {
      // Turning on — request permission
      const granted = await requestNotificationPermission();
      notificationPermission = getPermissionState();
      if (granted) {
        notificationsEnabled = true;
        setNotificationsEnabled(true);
      }
      // If denied, stay off
    } else {
      // Turning off
      notificationsEnabled = false;
      setNotificationsEnabled(false);
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

  async function saveUndoSendDelay() {
    undoSendSaving = true;
    try {
      const result = await api.config.setUndoSendDelay(undoSendDelay);
      undoSendDelay = result.delay_seconds;
    } catch {
      // Silently fail
    } finally {
      undoSendSaving = false;
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

  // Labels functions
  async function loadLabels() {
    try { labelList2 = await api.labels.list(); } catch { labelList2 = []; }
  }

  async function createLabel() {
    if (!labelNewName.trim() || labelSaving) return;
    labelSaving = true;
    try {
      await api.labels.create({ name: labelNewName.trim(), color: labelNewColor });
      labelNewName = '';
      labelNewColor = '#3B82F6';
      labelShowNew = false;
      await loadLabels();
    } catch { /* silently fail */ }
    finally { labelSaving = false; }
  }

  function startEditLabel(l: LabelItem) {
    labelEditing = l.id;
    labelEditName = l.name;
    labelEditColor = l.color;
  }

  function cancelEditLabel() {
    labelEditing = null;
  }

  async function saveEditLabel() {
    if (!labelEditing || labelSaving || !labelEditName.trim()) return;
    labelSaving = true;
    try {
      await api.labels.update(labelEditing, { name: labelEditName.trim(), color: labelEditColor });
      labelEditing = null;
      await loadLabels();
    } catch { /* silently fail */ }
    finally { labelSaving = false; }
  }

  async function deleteLabel(id: string) {
    try {
      await api.labels.delete(id);
      if (labelEditing === id) labelEditing = null;
      await loadLabels();
    } catch { /* silently fail */ }
  }

  // Filter Rules functions
  async function loadFilterRules() {
    try { filterRules = await api.filterRules.list(); } catch { filterRules = []; }
  }

  function addCondition(list: FilterCondition[]) {
    list.push({ field: 'from', operator: 'contains', value: '' });
  }

  function removeCondition(list: FilterCondition[], i: number) {
    list.splice(i, 1);
  }

  function addAction(list: FilterAction[]) {
    list.push({ type: 'archive' });
  }

  function removeAction(list: FilterAction[], i: number) {
    list.splice(i, 1);
  }

  async function createFilterRule() {
    if (!frNewName.trim() || frSaving) return;
    const validConditions = frNewConditions.filter(c => c.value.trim());
    if (validConditions.length === 0) return;
    frSaving = true;
    try {
      await api.filterRules.create({ name: frNewName.trim(), conditions: validConditions, actions: frNewActions });
      frNewName = '';
      frNewConditions = [{ field: 'from', operator: 'contains', value: '' }];
      frNewActions = [{ type: 'archive' }];
      frShowNew = false;
      await loadFilterRules();
    } catch { /* silently fail */ }
    finally { frSaving = false; }
  }

  function startEditRule(rule: FilterRuleItem) {
    frEditing = rule.id;
    frEditName = rule.name;
    frEditConditions = rule.conditions.map(c => ({ ...c }));
    frEditActions = rule.actions.map(a => ({ ...a }));
    frEditActive = rule.is_active;
  }

  function cancelEditRule() {
    frEditing = null;
  }

  async function saveEditRule() {
    if (!frEditing || frSaving || !frEditName.trim()) return;
    const validConditions = frEditConditions.filter(c => c.value.trim());
    if (validConditions.length === 0) return;
    frSaving = true;
    try {
      await api.filterRules.update(frEditing, { name: frEditName.trim(), conditions: validConditions, actions: frEditActions, is_active: frEditActive });
      frEditing = null;
      await loadFilterRules();
    } catch { /* silently fail */ }
    finally { frSaving = false; }
  }

  async function deleteFilterRule(id: string) {
    try {
      await api.filterRules.delete(id);
      if (frEditing === id) frEditing = null;
      await loadFilterRules();
    } catch { /* silently fail */ }
  }

  async function toggleRuleActive(rule: FilterRuleItem) {
    try {
      await api.filterRules.update(rule.id, { name: rule.name, conditions: rule.conditions, actions: rule.actions, is_active: !rule.is_active });
      await loadFilterRules();
    } catch { /* silently fail */ }
  }

  // Aliases functions
  async function loadAliases() {
    const acctId = aliasAccountId || (accounts.length > 0 ? accounts[0].id : '');
    if (!acctId) { aliases = []; return; }
    try { aliases = await api.aliases.list(acctId); } catch { aliases = []; }
  }

  async function createAlias() {
    if (!aliasNewEmail.trim() || aliasSaving) return;
    const acctId = aliasAccountId || (accounts.length > 0 ? accounts[0].id : '');
    if (!acctId) return;
    aliasSaving = true;
    try {
      await api.aliases.create({
        account_id: acctId,
        email: aliasNewEmail.trim(),
        display_name: aliasNewDisplayName.trim(),
        reply_to: aliasNewReplyTo.trim() || undefined,
        is_default: aliasNewDefault,
      });
      aliasNewEmail = '';
      aliasNewDisplayName = '';
      aliasNewReplyTo = '';
      aliasNewDefault = false;
      aliasShowNew = false;
      await loadAliases();
    } catch { /* silently fail */ }
    finally { aliasSaving = false; }
  }

  function startEditAlias(a: AliasItem) {
    aliasEditing = a.id;
    aliasEditEmail = a.email;
    aliasEditDisplayName = a.display_name;
    aliasEditReplyTo = a.reply_to || '';
    aliasEditDefault = a.is_default;
  }

  function cancelEditAlias() {
    aliasEditing = null;
  }

  async function saveEditAlias() {
    if (!aliasEditing || aliasSaving || !aliasEditEmail.trim()) return;
    aliasSaving = true;
    try {
      await api.aliases.update(aliasEditing, {
        email: aliasEditEmail.trim(),
        display_name: aliasEditDisplayName.trim(),
        reply_to: aliasEditReplyTo.trim() || undefined,
        is_default: aliasEditDefault,
      });
      aliasEditing = null;
      await loadAliases();
    } catch { /* silently fail */ }
    finally { aliasSaving = false; }
  }

  async function deleteAlias(id: string) {
    try {
      await api.aliases.delete(id);
      if (aliasEditing === id) aliasEditing = null;
      await loadAliases();
    } catch { /* silently fail */ }
  }

  async function loadAccounts() {
    try {
      accounts = await api.accounts.list();
      if (accounts.length > 0 && !sigAccountId) {
        sigAccountId = accounts[0].id;
        await loadSignatures();
      }
      if (accounts.length > 0 && !aliasAccountId) {
        aliasAccountId = accounts[0].id;
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

  async function loadBlockedSenders() {
    try {
      blockedSenders = await api.blockedSenders.list();
    } catch { blockedSenders = []; }
  }

  async function blockEmail() {
    if (!newBlockEmail.trim() || blockingEmail) return;
    blockingEmail = true;
    try {
      await api.blockedSenders.block({ email_address: newBlockEmail.trim() });
      newBlockEmail = '';
      await loadBlockedSenders();
    } catch { /* silently fail */ }
    finally { blockingEmail = false; }
  }

  async function unblockSender(id: string) {
    try {
      await api.blockedSenders.unblock(id);
      await loadBlockedSenders();
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

  async function loadAccountNotifications() {
    try {
      for (const account of accounts) {
        try {
          const res = await api.notifications.get(account.id);
          acctNotificationState[account.id] = res.enabled;
        } catch {
          acctNotificationState[account.id] = true; // default enabled
        }
      }
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

  async function loadAuditLog() {
    try {
      auditEntries = await api.auditLog.list({ limit: 25 });
    } catch { auditEntries = []; }
  }

  async function runSubscriptionAudit() {
    auditRunning = true;
    try {
      const result = await api.subscriptions.audit();
      subscriptions = result.subscriptions;
      auditLoaded = true;
    } catch {
      subscriptions = [];
      auditLoaded = true;
    } finally {
      auditRunning = false;
    }
  }

  function readRateColor(rate: number): string {
    if (rate < 0.2) return 'var(--iris-color-error)';
    if (rate < 0.5) return 'var(--iris-color-warning)';
    return 'var(--iris-color-success)';
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
        decayEnabled = aiConfig.decay_enabled ?? true;
        decayThresholdDays = aiConfig.decay_threshold_days ?? 7;
      } catch {
        // AI config not available
      }

      try {
        const undoConfig = await api.config.getUndoSendDelay();
        undoSendDelay = undoConfig.delay_seconds;
      } catch {
        undoSendDelay = 10;
      }

      await loadAccounts();
      await loadAccountNotifications();
      await loadTemplates();
      await loadLabels();
      await loadFilterRules();
      await loadBlockedSenders();
      await loadApiKeys();
      await loadAuditLog();
      await loadAliases();

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

      <!-- Desktop Notifications -->
      <div class="flex items-center justify-between mt-6 p-3 rounded-lg border" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface);">
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
        <label class="inline-flex items-center gap-2 cursor-pointer">
          <input
            type="checkbox"
            checked={notificationsEnabled && notificationPermission === 'granted'}
            disabled={notificationPermission === 'unsupported' || notificationPermission === 'denied'}
            onchange={toggleNotifications}
            class="w-4 h-4 rounded"
            style="accent-color: var(--iris-color-primary);"
          />
        </label>
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
          <select
            bind:value={undoSendDelay}
            onchange={saveUndoSendDelay}
            class="settings-input px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
            disabled={undoSendSaving}
          >
            <option value={5}>5 seconds</option>
            <option value={10}>10 seconds</option>
            <option value={15}>15 seconds</option>
            <option value={20}>20 seconds</option>
            <option value={30}>30 seconds</option>
          </select>
          {#if undoSendSaving}
            <span class="text-xs" style="color: var(--iris-color-text-faint);">Saving...</span>
          {/if}
        </div>
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

    <!-- Templates section -->
    <section>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Templates</h3>
      <p class="text-xs mb-4" style="color: var(--iris-color-text-faint);">Create reusable email templates for quick composing.</p>

      {#if editingTemplateId}
        <div class="p-3 rounded-lg border mb-4" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface);">
          <div class="space-y-2">
            <input
              type="text"
              bind:value={templateName}
              placeholder="Template name"
              class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
              style="border-color: var(--iris-color-border); background: var(--iris-color-bg); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
            />
            <input
              type="text"
              bind:value={templateSubject}
              placeholder="Subject (optional)"
              class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
              style="border-color: var(--iris-color-border); background: var(--iris-color-bg); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
            />
            <textarea
              bind:value={templateBody}
              placeholder="Template body"
              rows="4"
              class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2 resize-y"
              style="border-color: var(--iris-color-border); background: var(--iris-color-bg); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
            ></textarea>
            <div class="flex gap-2">
              <button
                class="settings-btn-primary px-4 py-1.5 text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
                style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
                onclick={saveTemplate}
                disabled={templateSaving || !templateName.trim() || !templateBody.trim()}
              >
                {templateSaving ? 'Saving...' : editingTemplateId === 'new' ? 'Create' : 'Save'}
              </button>
              <button
                class="settings-btn-secondary px-4 py-1.5 text-sm rounded-lg border transition-colors"
                style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
                onclick={cancelEditTemplate}
              >Cancel</button>
            </div>
          </div>
        </div>
      {:else}
        <button
          class="settings-btn-primary px-4 py-2 text-sm font-medium rounded-lg transition-colors mb-4"
          style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
          onclick={startNewTemplate}
        >New Template</button>
      {/if}

      {#if templates.length > 0}
        <div class="border rounded-lg overflow-hidden" style="border-color: var(--iris-color-border);">
          <table class="w-full text-sm">
            <thead style="background: var(--iris-color-bg-surface);">
              <tr>
                <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Name</th>
                <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Subject</th>
                <th class="text-right px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);"></th>
              </tr>
            </thead>
            <tbody>
              {#each templates as t}
                <tr class="border-t" style="border-color: var(--iris-color-border-subtle);">
                  <td class="px-3 py-2 font-medium" style="color: var(--iris-color-text);">{t.name}</td>
                  <td class="px-3 py-2 text-xs" style="color: var(--iris-color-text-faint);">{t.subject || '—'}</td>
                  <td class="px-3 py-2 text-right">
                    <button
                      class="text-xs mr-2 settings-edit-btn"
                      style="color: var(--iris-color-primary);"
                      onclick={() => startEditTemplate(t)}
                    >Edit</button>
                    <button
                      class="text-xs settings-revoke-btn"
                      style="color: var(--iris-color-error);"
                      onclick={() => deleteTemplate(t.id)}
                    >Delete</button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else if !editingTemplateId}
        <p class="text-sm" style="color: var(--iris-color-text-faint);">No templates created yet.</p>
      {/if}
    </section>

    <!-- Labels section -->
    <section>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Labels</h3>
      <p class="text-xs mb-4" style="color: var(--iris-color-text-faint);">Organize emails with colored labels. Apply labels from inbox actions or filter rules.</p>

      <!-- Existing labels -->
      {#if labelList2.length > 0}
        <div class="space-y-2 mb-4">
          {#each labelList2 as label}
            <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
              {#if labelEditing === label.id}
                <!-- Edit mode -->
                <div class="space-y-2">
                  <input type="text" bind:value={labelEditName} placeholder="Label name"
                    class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
                    style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);" />
                  <div class="flex items-center gap-2">
                    <span class="text-xs" style="color: var(--iris-color-text-faint);">Color:</span>
                    {#each LABEL_COLORS as c}
                      <button
                        class="w-5 h-5 rounded-full border-2 transition-transform"
                        style="background: {c}; border-color: {labelEditColor === c ? 'var(--iris-color-text)' : 'transparent'}; transform: {labelEditColor === c ? 'scale(1.2)' : 'scale(1)'};"
                        onclick={() => (labelEditColor = c)}
                      ></button>
                    {/each}
                  </div>
                  <div class="flex items-center gap-2">
                    <span class="flex-1"></span>
                    <button class="settings-btn-secondary px-3 py-1.5 text-xs rounded-lg border transition-colors"
                      style="border-color: var(--iris-color-border); color: var(--iris-color-text);" onclick={cancelEditLabel}>Cancel</button>
                    <button class="settings-btn-primary px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-50"
                      style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
                      onclick={saveEditLabel} disabled={labelSaving || !labelEditName.trim()}>{labelSaving ? 'Saving...' : 'Save'}</button>
                  </div>
                </div>
              {:else}
                <!-- Display mode -->
                <div class="flex items-center gap-3">
                  <div class="w-3.5 h-3.5 rounded-full shrink-0" style="background: {label.color};"></div>
                  <span class="text-sm font-medium flex-1" style="color: var(--iris-color-text);">{label.name}</span>
                  <span class="text-xs" style="color: var(--iris-color-text-faint);">{label.message_count} message{label.message_count !== 1 ? 's' : ''}</span>
                  <button class="settings-btn-secondary px-2 py-1 text-xs rounded border transition-colors"
                    style="border-color: var(--iris-color-border); color: var(--iris-color-text-muted);"
                    onclick={() => startEditLabel(label)}>Edit</button>
                  <button class="settings-revoke-btn px-2 py-1 text-xs rounded transition-colors"
                    style="color: var(--iris-color-error);" onclick={() => deleteLabel(label.id)}>Delete</button>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {:else if !labelShowNew}
        <p class="text-sm mb-4" style="color: var(--iris-color-text-faint);">No labels created yet.</p>
      {/if}

      <!-- New label form -->
      {#if labelShowNew}
        <div class="p-3 rounded-lg border space-y-2" style="border-color: var(--iris-color-border);">
          <input type="text" bind:value={labelNewName} placeholder="Label name (e.g., Work, Personal)"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);" />
          <div class="flex items-center gap-2">
            <span class="text-xs" style="color: var(--iris-color-text-faint);">Color:</span>
            {#each LABEL_COLORS as c}
              <button
                class="w-5 h-5 rounded-full border-2 transition-transform"
                style="background: {c}; border-color: {labelNewColor === c ? 'var(--iris-color-text)' : 'transparent'}; transform: {labelNewColor === c ? 'scale(1.2)' : 'scale(1)'};"
                onclick={() => (labelNewColor = c)}
              ></button>
            {/each}
          </div>
          <div class="flex gap-2">
            <button class="settings-btn-secondary px-3 py-1.5 text-xs rounded-lg border transition-colors"
              style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
              onclick={() => { labelShowNew = false; labelNewName = ''; labelNewColor = '#3B82F6'; }}>Cancel</button>
            <button class="settings-btn-primary px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-50"
              style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
              onclick={createLabel} disabled={labelSaving || !labelNewName.trim()}>{labelSaving ? 'Creating...' : 'Create Label'}</button>
          </div>
        </div>
      {:else}
        <button class="settings-btn-secondary px-4 py-2 text-sm rounded-lg border transition-colors"
          style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
          onclick={() => (labelShowNew = true)}>+ Add Label</button>
      {/if}
    </section>

    <!-- Filter Rules section -->
    <section>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Filter Rules</h3>
      <p class="text-xs mb-4" style="color: var(--iris-color-text-faint);">Auto-apply actions to incoming emails based on conditions.</p>

      <!-- Existing rules -->
      {#if filterRules.length > 0}
        <div class="space-y-3 mb-4">
          {#each filterRules as rule}
            <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
              {#if frEditing === rule.id}
                <!-- Edit mode -->
                <div class="space-y-3">
                  <input type="text" bind:value={frEditName} placeholder="Rule name"
                    class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
                    style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);" />

                  <div>
                    <p class="text-xs font-medium mb-2" style="color: var(--iris-color-text-muted);">When ALL conditions match:</p>
                    {#each frEditConditions as cond, i}
                      <div class="flex gap-2 mb-2">
                        <select bind:value={cond.field} class="settings-input px-2 py-1.5 rounded border text-xs" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                          <option value="from">From</option><option value="to">To</option><option value="subject">Subject</option>
                          <option value="category">Category</option><option value="is_read">Is Read</option><option value="has_attachments">Has Attachments</option>
                        </select>
                        <select bind:value={cond.operator} class="settings-input px-2 py-1.5 rounded border text-xs" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                          <option value="contains">contains</option><option value="equals">equals</option>
                          <option value="starts_with">starts with</option><option value="ends_with">ends with</option>
                        </select>
                        <input type="text" bind:value={cond.value} placeholder="Value"
                          class="settings-input flex-1 px-2 py-1.5 rounded border text-xs focus:outline-none"
                          style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);" />
                        {#if frEditConditions.length > 1}
                          <button class="text-xs" style="color: var(--iris-color-error);" onclick={() => removeCondition(frEditConditions, i)}>x</button>
                        {/if}
                      </div>
                    {/each}
                    <button class="text-xs" style="color: var(--iris-color-primary);" onclick={() => addCondition(frEditConditions)}>+ Add condition</button>
                  </div>

                  <div>
                    <p class="text-xs font-medium mb-2" style="color: var(--iris-color-text-muted);">Then:</p>
                    {#each frEditActions as act, i}
                      <div class="flex gap-2 mb-2">
                        <select bind:value={act.type} class="settings-input px-2 py-1.5 rounded border text-xs" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                          <option value="archive">Archive</option><option value="delete">Delete</option><option value="mark_read">Mark Read</option>
                          <option value="star">Star</option><option value="label">Add Label</option>
                        </select>
                        {#if act.type === 'label'}
                          <input type="text" bind:value={act.value} placeholder="Label name"
                            class="settings-input flex-1 px-2 py-1.5 rounded border text-xs focus:outline-none"
                            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);" />
                        {/if}
                        {#if frEditActions.length > 1}
                          <button class="text-xs" style="color: var(--iris-color-error);" onclick={() => removeAction(frEditActions, i)}>x</button>
                        {/if}
                      </div>
                    {/each}
                    <button class="text-xs" style="color: var(--iris-color-primary);" onclick={() => addAction(frEditActions)}>+ Add action</button>
                  </div>

                  <div class="flex items-center gap-3">
                    <label class="flex items-center gap-2 text-xs cursor-pointer" style="color: var(--iris-color-text);">
                      <input type="checkbox" bind:checked={frEditActive} class="rounded" /> Active
                    </label>
                    <span class="flex-1"></span>
                    <button class="settings-btn-secondary px-3 py-1.5 text-xs rounded-lg border transition-colors"
                      style="border-color: var(--iris-color-border); color: var(--iris-color-text);" onclick={cancelEditRule}>Cancel</button>
                    <button class="settings-btn-primary px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-50"
                      style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
                      onclick={saveEditRule} disabled={frSaving || !frEditName.trim()}>{frSaving ? 'Saving...' : 'Save'}</button>
                  </div>
                </div>
              {:else}
                <!-- Display mode -->
                <div class="flex items-start justify-between gap-3">
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2 mb-1">
                      <span class="text-sm font-medium" style="color: var(--iris-color-text);">{rule.name}</span>
                      <span class="px-1.5 py-0.5 text-[10px] rounded-full font-medium"
                        style="background: {rule.is_active ? 'color-mix(in srgb, var(--iris-color-success) 20%, transparent)' : 'color-mix(in srgb, var(--iris-color-text-faint) 20%, transparent)'}; color: {rule.is_active ? 'var(--iris-color-success)' : 'var(--iris-color-text-faint)'};">
                        {rule.is_active ? 'Active' : 'Paused'}
                      </span>
                    </div>
                    <div class="flex flex-wrap gap-1 mb-1">
                      {#each rule.conditions as cond}
                        <span class="px-1.5 py-0.5 text-[10px] rounded" style="background: color-mix(in srgb, var(--iris-color-info) 15%, transparent); color: var(--iris-color-info);">
                          {cond.field} {cond.operator} "{cond.value}"
                        </span>
                      {/each}
                    </div>
                    <div class="flex flex-wrap gap-1">
                      {#each rule.actions as act}
                        <span class="px-1.5 py-0.5 text-[10px] rounded" style="background: color-mix(in srgb, var(--iris-color-warning) 15%, transparent); color: var(--iris-color-warning);">
                          {act.type}{act.value ? `: ${act.value}` : ''}
                        </span>
                      {/each}
                    </div>
                  </div>
                  <div class="flex items-center gap-1 shrink-0">
                    <button class="settings-btn-secondary px-2 py-1 text-xs rounded border transition-colors"
                      style="border-color: var(--iris-color-border); color: var(--iris-color-text-muted);"
                      onclick={() => toggleRuleActive(rule)}>{rule.is_active ? 'Pause' : 'Resume'}</button>
                    <button class="settings-btn-secondary px-2 py-1 text-xs rounded border transition-colors"
                      style="border-color: var(--iris-color-border); color: var(--iris-color-text-muted);"
                      onclick={() => startEditRule(rule)}>Edit</button>
                    <button class="settings-revoke-btn px-2 py-1 text-xs rounded transition-colors"
                      style="color: var(--iris-color-error);" onclick={() => deleteFilterRule(rule.id)}>Delete</button>
                  </div>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {:else if !frShowNew}
        <p class="text-sm mb-4" style="color: var(--iris-color-text-faint);">No filter rules created yet.</p>
      {/if}

      <!-- New rule form -->
      {#if frShowNew}
        <div class="p-3 rounded-lg border space-y-3" style="border-color: var(--iris-color-border);">
          <input type="text" bind:value={frNewName} placeholder="Rule name (e.g., Auto-archive newsletters)"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);" />

          <div>
            <p class="text-xs font-medium mb-2" style="color: var(--iris-color-text-muted);">When ALL conditions match:</p>
            {#each frNewConditions as cond, i}
              <div class="flex gap-2 mb-2">
                <select bind:value={cond.field} class="settings-input px-2 py-1.5 rounded border text-xs" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                  <option value="from">From</option><option value="to">To</option><option value="subject">Subject</option>
                  <option value="category">Category</option><option value="is_read">Is Read</option><option value="has_attachments">Has Attachments</option>
                </select>
                <select bind:value={cond.operator} class="settings-input px-2 py-1.5 rounded border text-xs" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                  <option value="contains">contains</option><option value="equals">equals</option>
                  <option value="starts_with">starts with</option><option value="ends_with">ends with</option>
                </select>
                <input type="text" bind:value={cond.value} placeholder="Value"
                  class="settings-input flex-1 px-2 py-1.5 rounded border text-xs focus:outline-none"
                  style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);" />
                {#if frNewConditions.length > 1}
                  <button class="text-xs" style="color: var(--iris-color-error);" onclick={() => removeCondition(frNewConditions, i)}>x</button>
                {/if}
              </div>
            {/each}
            <button class="text-xs" style="color: var(--iris-color-primary);" onclick={() => addCondition(frNewConditions)}>+ Add condition</button>
          </div>

          <div>
            <p class="text-xs font-medium mb-2" style="color: var(--iris-color-text-muted);">Then:</p>
            {#each frNewActions as act, i}
              <div class="flex gap-2 mb-2">
                <select bind:value={act.type} class="settings-input px-2 py-1.5 rounded border text-xs" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                  <option value="archive">Archive</option><option value="delete">Delete</option><option value="mark_read">Mark Read</option>
                  <option value="star">Star</option><option value="label">Add Label</option>
                </select>
                {#if act.type === 'label'}
                  <input type="text" bind:value={act.value} placeholder="Label name"
                    class="settings-input flex-1 px-2 py-1.5 rounded border text-xs focus:outline-none"
                    style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);" />
                {/if}
                {#if frNewActions.length > 1}
                  <button class="text-xs" style="color: var(--iris-color-error);" onclick={() => removeAction(frNewActions, i)}>x</button>
                {/if}
              </div>
            {/each}
            <button class="text-xs" style="color: var(--iris-color-primary);" onclick={() => addAction(frNewActions)}>+ Add action</button>
          </div>

          <div class="flex gap-2">
            <button class="settings-btn-secondary px-3 py-1.5 text-xs rounded-lg border transition-colors"
              style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
              onclick={() => { frShowNew = false; frNewName = ''; frNewConditions = [{ field: 'from', operator: 'contains', value: '' }]; frNewActions = [{ type: 'archive' }]; }}>Cancel</button>
            <button class="settings-btn-primary px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-50"
              style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
              onclick={createFilterRule} disabled={frSaving || !frNewName.trim()}>{frSaving ? 'Creating...' : 'Create Rule'}</button>
          </div>
        </div>
      {:else}
        <button class="settings-btn-secondary px-4 py-2 text-sm rounded-lg border transition-colors"
          style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
          onclick={() => (frShowNew = true)}>+ Add Filter Rule</button>
      {/if}
    </section>

    <!-- Send-as Aliases section -->
    <section>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Send-as Aliases</h3>
      <p class="text-xs mb-4" style="color: var(--iris-color-text-faint);">Alternative sender identities for composing emails. The default alias is used in the "From" field.</p>

      <!-- Account picker -->
      {#if accounts.length > 1}
        <div class="mb-4">
          <select bind:value={aliasAccountId}
            onchange={() => loadAliases()}
            class="settings-input px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);">
            {#each accounts as acct}
              <option value={acct.id}>{acct.email}</option>
            {/each}
          </select>
        </div>
      {:else if accounts.length === 1}
        <p class="text-xs mb-4" style="color: var(--iris-color-text-muted);">Account: {accounts[0].email}</p>
      {/if}

      <!-- Existing aliases -->
      {#if aliases.length > 0}
        <div class="space-y-3 mb-4">
          {#each aliases as a}
            <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
              {#if aliasEditing === a.id}
                <!-- Edit mode -->
                <div class="space-y-2">
                  <input type="email" bind:value={aliasEditEmail} placeholder="Email address"
                    class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
                    style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);" />
                  <input type="text" bind:value={aliasEditDisplayName} placeholder="Display name (e.g., John Doe)"
                    class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
                    style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);" />
                  <input type="email" bind:value={aliasEditReplyTo} placeholder="Reply-to address (optional, defaults to alias email)"
                    class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
                    style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);" />
                  <div class="flex items-center gap-3">
                    <label class="flex items-center gap-2 text-sm cursor-pointer" style="color: var(--iris-color-text);">
                      <input type="checkbox" bind:checked={aliasEditDefault} class="rounded" /> Default
                    </label>
                    <span class="flex-1"></span>
                    <button class="settings-btn-secondary px-3 py-1.5 text-xs rounded-lg border transition-colors"
                      style="border-color: var(--iris-color-border); color: var(--iris-color-text);" onclick={cancelEditAlias}>Cancel</button>
                    <button class="settings-btn-primary px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-50"
                      style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
                      onclick={saveEditAlias} disabled={aliasSaving || !aliasEditEmail.trim()}>{aliasSaving ? 'Saving...' : 'Save'}</button>
                  </div>
                </div>
              {:else}
                <!-- Display mode -->
                <div class="flex items-start justify-between gap-3">
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2 mb-1">
                      <span class="text-sm font-medium" style="color: var(--iris-color-text);">{a.email}</span>
                      {#if a.is_default}
                        <span class="px-1.5 py-0.5 text-[10px] rounded-full font-medium" style="background: color-mix(in srgb, var(--iris-color-primary) 20%, transparent); color: var(--iris-color-primary);">default</span>
                      {/if}
                    </div>
                    {#if a.display_name}
                      <p class="text-xs" style="color: var(--iris-color-text-faint);">Display: {a.display_name}</p>
                    {/if}
                    {#if a.reply_to}
                      <p class="text-xs" style="color: var(--iris-color-text-faint);">Reply-to: {a.reply_to}</p>
                    {/if}
                  </div>
                  <div class="flex items-center gap-1 shrink-0">
                    <button class="settings-btn-secondary px-2 py-1 text-xs rounded border transition-colors"
                      style="border-color: var(--iris-color-border); color: var(--iris-color-text-muted);"
                      onclick={() => startEditAlias(a)}>Edit</button>
                    <button class="settings-revoke-btn px-2 py-1 text-xs rounded transition-colors"
                      style="color: var(--iris-color-error);" onclick={() => deleteAlias(a.id)}>Delete</button>
                  </div>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {:else if aliasAccountId || accounts.length > 0}
        <p class="text-sm mb-4" style="color: var(--iris-color-text-faint);">No aliases for this account.</p>
      {/if}

      <!-- Add new alias -->
      {#if aliasShowNew}
        <div class="p-3 rounded-lg border space-y-2" style="border-color: var(--iris-color-border);">
          <input type="email" bind:value={aliasNewEmail} placeholder="Email address (e.g., support@myproduct.io)"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);" />
          <input type="text" bind:value={aliasNewDisplayName} placeholder="Display name (e.g., Support Team)"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);" />
          <input type="email" bind:value={aliasNewReplyTo} placeholder="Reply-to address (optional)"
            class="settings-input w-full px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
            style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);" />
          <div class="flex items-center gap-3">
            <label class="flex items-center gap-2 text-sm cursor-pointer" style="color: var(--iris-color-text);">
              <input type="checkbox" bind:checked={aliasNewDefault} class="rounded" /> Set as default
            </label>
            <span class="flex-1"></span>
            <button class="settings-btn-secondary px-3 py-1.5 text-xs rounded-lg border transition-colors"
              style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
              onclick={() => { aliasShowNew = false; aliasNewEmail = ''; aliasNewDisplayName = ''; aliasNewReplyTo = ''; aliasNewDefault = false; }}>Cancel</button>
            <button class="settings-btn-primary px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-50"
              style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
              onclick={createAlias} disabled={aliasSaving || !aliasNewEmail.trim()}>{aliasSaving ? 'Creating...' : 'Create'}</button>
          </div>
        </div>
      {:else}
        <button class="settings-btn-secondary px-4 py-2 text-sm rounded-lg border transition-colors disabled:opacity-50"
          style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
          onclick={() => (aliasShowNew = true)}
          disabled={!aliasAccountId && accounts.length === 0}>+ Add Alias</button>
      {/if}
    </section>

    <!-- Blocked Senders section -->
    <section>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Blocked Senders</h3>
      <p class="text-xs mb-4" style="color: var(--iris-color-text-faint);">Emails from blocked senders are automatically moved to Spam.</p>

      <div class="flex gap-2 mb-4">
        <input
          type="email"
          bind:value={newBlockEmail}
          placeholder="Block a sender manually (email address)"
          class="settings-input flex-1 px-3 py-2 rounded-lg border text-sm focus:outline-none focus:ring-2"
          style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text); --tw-ring-color: var(--iris-color-primary);"
          onkeydown={(e) => { if (e.key === 'Enter') blockEmail(); }}
        />
        <button
          class="settings-btn-primary px-4 py-2 text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
          style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
          onclick={blockEmail}
          disabled={blockingEmail || !newBlockEmail.trim()}
        >
          {blockingEmail ? 'Blocking...' : 'Block'}
        </button>
      </div>

      {#if blockedSenders.length > 0}
        <div class="border rounded-lg overflow-hidden" style="border-color: var(--iris-color-border);">
          <table class="w-full text-sm">
            <thead style="background: var(--iris-color-bg-surface);">
              <tr>
                <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Email</th>
                <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Reason</th>
                <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Blocked</th>
                <th class="text-right px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);"></th>
              </tr>
            </thead>
            <tbody>
              {#each blockedSenders as sender}
                <tr class="border-t" style="border-color: var(--iris-color-border-subtle);">
                  <td class="px-3 py-2 font-medium" style="color: var(--iris-color-text);">{sender.email_address}</td>
                  <td class="px-3 py-2 text-xs" style="color: var(--iris-color-text-faint);">{sender.reason || '--'}</td>
                  <td class="px-3 py-2 text-xs" style="color: var(--iris-color-text-faint);">
                    {formatTimestamp(sender.created_at)}
                  </td>
                  <td class="px-3 py-2 text-right">
                    <button
                      class="settings-revoke-btn text-xs"
                      style="color: var(--iris-color-error);"
                      onclick={() => unblockSender(sender.id)}
                    >Unblock</button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p class="text-sm" style="color: var(--iris-color-text-faint);">No blocked senders.</p>
      {/if}
    </section>

    <!-- Subscription Audit section -->
    <section>
      <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Subscription Audit</h3>
      <p class="text-xs mb-4" style="color: var(--iris-color-text-faint);">Find mailing lists you never read. Senders with 3+ emails are analyzed by read rate.</p>

      <button
        class="settings-btn-primary px-4 py-2 text-sm font-medium rounded-lg transition-colors disabled:opacity-50 mb-4"
        style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
        onclick={runSubscriptionAudit}
        disabled={auditRunning}
      >
        {auditRunning ? 'Analyzing...' : 'Run Audit'}
      </button>

      {#if auditLoaded}
        {#if subscriptions.length > 0}
          <div class="border rounded-lg overflow-hidden" style="border-color: var(--iris-color-border);">
            <table class="w-full text-sm">
              <thead style="background: var(--iris-color-bg-surface);">
                <tr>
                  <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Sender</th>
                  <th class="text-right px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Emails</th>
                  <th class="text-right px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Read Rate</th>
                  <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Last Received</th>
                  <th class="text-left px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Category</th>
                  <th class="text-right px-3 py-2 text-xs font-medium" style="color: var(--iris-color-text-muted);">Unsub</th>
                </tr>
              </thead>
              <tbody>
                {#each subscriptions as sub}
                  <tr class="border-t" style="border-color: var(--iris-color-border-subtle);">
                    <td class="px-3 py-2" style="color: var(--iris-color-text);">
                      <div class="font-medium text-xs">{sub.sender_name || sub.sender}</div>
                      {#if sub.sender_name}
                        <div class="text-[10px]" style="color: var(--iris-color-text-faint);">{sub.sender}</div>
                      {/if}
                    </td>
                    <td class="px-3 py-2 text-right text-xs tabular-nums" style="color: var(--iris-color-text);">
                      {sub.total_count}
                    </td>
                    <td class="px-3 py-2 text-right">
                      <span class="text-xs font-semibold tabular-nums" style="color: {readRateColor(sub.read_rate)};">
                        {Math.round(sub.read_rate * 100)}%
                      </span>
                    </td>
                    <td class="px-3 py-2 text-xs" style="color: var(--iris-color-text-faint);">
                      {formatTimestamp(sub.last_received)}
                    </td>
                    <td class="px-3 py-2">
                      {#if sub.category}
                        <span class="px-2 py-0.5 text-[10px] rounded-full" style="background: var(--iris-color-bg-surface); color: var(--iris-color-text-muted);">
                          {sub.category}
                        </span>
                      {:else}
                        <span class="text-xs" style="color: var(--iris-color-text-faint);">--</span>
                      {/if}
                    </td>
                    <td class="px-3 py-2 text-right">
                      {#if sub.has_unsubscribe}
                        <span class="px-1.5 py-0.5 rounded text-[10px] font-medium" style="background: color-mix(in srgb, var(--iris-color-info) 15%, transparent); color: var(--iris-color-info);">Available</span>
                      {:else}
                        <span class="text-[10px]" style="color: var(--iris-color-text-faint);">--</span>
                      {/if}
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          <p class="text-sm" style="color: var(--iris-color-text-faint);">No recurring senders found. Your inbox is clean!</p>
        {/if}
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
    background: var(--iris-color-primary-hover);
  }

  .settings-btn-secondary:hover:not(:disabled) {
    background: var(--iris-color-bg-surface);
  }

  .settings-revoke-btn:hover {
    background: color-mix(in srgb, var(--iris-color-error) 12%, transparent);
  }

  .settings-edit-btn:hover {
    background: var(--iris-color-bg-hover);
  }

  .settings-input::placeholder {
    color: var(--iris-color-text-faint);
  }
</style>
