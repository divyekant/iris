<script lang="ts">
  import { api } from '../../lib/api';
  import FormInput from '../shared/FormInput.svelte';
  import FormToggle from '../shared/FormToggle.svelte';

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

  // AI Categories
  type CustomCategoryItem = { id: string; account_id: string; name: string; description: string | null; is_ai_generated: boolean; email_count: number; status: string; created_at: number; updated_at: number };
  let customCategories = $state<CustomCategoryItem[]>([]);
  let catShowNew = $state(false);
  let catNewName = $state('');
  let catNewDescription = $state('');
  let catSaving = $state(false);
  let catAnalyzing = $state(false);
  let catAnalyzeMessage = $state('');

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

  // AI Categories functions
  async function loadCustomCategories() {
    try { customCategories = await api.customCategories.list(); } catch { customCategories = []; }
  }

  async function createCustomCategory() {
    if (!catNewName.trim() || catSaving) return;
    catSaving = true;
    try {
      const accounts = await api.accounts.list();
      if (accounts.length === 0) return;
      await api.customCategories.create({
        account_id: accounts[0].id,
        name: catNewName.trim(),
        description: catNewDescription.trim() || undefined,
      });
      catNewName = '';
      catNewDescription = '';
      catShowNew = false;
      await loadCustomCategories();
    } catch { /* silently fail */ }
    finally { catSaving = false; }
  }

  async function deleteCustomCategory(id: string) {
    try {
      await api.customCategories.delete(id);
      await loadCustomCategories();
    } catch { /* silently fail */ }
  }

  async function acceptCategory(id: string) {
    try {
      await api.customCategories.accept(id);
      await loadCustomCategories();
    } catch { /* silently fail */ }
  }

  async function dismissCategory(id: string) {
    try {
      await api.customCategories.dismiss(id);
      await loadCustomCategories();
    } catch { /* silently fail */ }
  }

  async function analyzeCategories() {
    catAnalyzing = true;
    catAnalyzeMessage = '';
    try {
      const accounts = await api.accounts.list();
      if (accounts.length === 0) { catAnalyzeMessage = 'No accounts found'; return; }
      const result = await api.customCategories.analyze(accounts[0].id);
      catAnalyzeMessage = result.suggested.length > 0
        ? `Found ${result.suggested.length} suggestion${result.suggested.length === 1 ? '' : 's'} from ${result.analyzed_messages} emails`
        : `No new categories suggested (analyzed ${result.analyzed_messages} emails)`;
      await loadCustomCategories();
    } catch {
      catAnalyzeMessage = 'Analysis failed';
    } finally {
      catAnalyzing = false;
      setTimeout(() => { catAnalyzeMessage = ''; }, 5000);
    }
  }

  // Load data on mount
  $effect(() => {
    async function loadData() {
      await loadLabels();
      await loadFilterRules();
      await loadCustomCategories();
    }
    loadData();
  });
</script>

<div class="space-y-8">
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
                <FormInput bind:value={labelEditName} placeholder="Label name" />
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
        <FormInput bind:value={labelNewName} placeholder="Label name (e.g., Work, Personal)" />
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
                <FormInput bind:value={frEditName} placeholder="Rule name" />

                <div>
                  <p class="text-xs font-medium mb-2" style="color: var(--iris-color-text-muted);">When ALL conditions match:</p>
                  {#each frEditConditions as cond, i}
                    <div class="flex gap-2 mb-2">
                      <select bind:value={cond.field} class="inline-select px-2 py-1.5 rounded border text-xs" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                        <option value="from">From</option><option value="to">To</option><option value="subject">Subject</option>
                        <option value="category">Category</option><option value="is_read">Is Read</option><option value="has_attachments">Has Attachments</option>
                      </select>
                      <select bind:value={cond.operator} class="inline-select px-2 py-1.5 rounded border text-xs" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                        <option value="contains">contains</option><option value="equals">equals</option>
                        <option value="starts_with">starts with</option><option value="ends_with">ends with</option>
                      </select>
                      <div class="flex-1">
                        <FormInput bind:value={cond.value} placeholder="Value" />
                      </div>
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
                      <select bind:value={act.type} class="inline-select px-2 py-1.5 rounded border text-xs" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                        <option value="archive">Archive</option><option value="delete">Delete</option><option value="mark_read">Mark Read</option>
                        <option value="star">Star</option><option value="label">Add Label</option>
                      </select>
                      {#if act.type === 'label'}
                        <div class="flex-1">
                          <FormInput bind:value={act.value} placeholder="Label name" />
                        </div>
                      {/if}
                      {#if frEditActions.length > 1}
                        <button class="text-xs" style="color: var(--iris-color-error);" onclick={() => removeAction(frEditActions, i)}>x</button>
                      {/if}
                    </div>
                  {/each}
                  <button class="text-xs" style="color: var(--iris-color-primary);" onclick={() => addAction(frEditActions)}>+ Add action</button>
                </div>

                <div class="flex items-center gap-3">
                  <FormToggle label="Active" bind:checked={frEditActive} />
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
        <FormInput bind:value={frNewName} placeholder="Rule name (e.g., Auto-archive newsletters)" />

        <div>
          <p class="text-xs font-medium mb-2" style="color: var(--iris-color-text-muted);">When ALL conditions match:</p>
          {#each frNewConditions as cond, i}
            <div class="flex gap-2 mb-2">
              <select bind:value={cond.field} class="inline-select px-2 py-1.5 rounded border text-xs" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                <option value="from">From</option><option value="to">To</option><option value="subject">Subject</option>
                <option value="category">Category</option><option value="is_read">Is Read</option><option value="has_attachments">Has Attachments</option>
              </select>
              <select bind:value={cond.operator} class="inline-select px-2 py-1.5 rounded border text-xs" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                <option value="contains">contains</option><option value="equals">equals</option>
                <option value="starts_with">starts with</option><option value="ends_with">ends with</option>
              </select>
              <div class="flex-1">
                <FormInput bind:value={cond.value} placeholder="Value" />
              </div>
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
              <select bind:value={act.type} class="inline-select px-2 py-1.5 rounded border text-xs" style="border-color: var(--iris-color-border); background: var(--iris-color-bg-surface); color: var(--iris-color-text);">
                <option value="archive">Archive</option><option value="delete">Delete</option><option value="mark_read">Mark Read</option>
                <option value="star">Star</option><option value="label">Add Label</option>
              </select>
              {#if act.type === 'label'}
                <div class="flex-1">
                  <FormInput bind:value={act.value} placeholder="Label name" />
                </div>
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

  <!-- AI Categories section -->
  <section>
    <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">AI Categories</h3>
    <p class="text-xs mb-4" style="color: var(--iris-color-text-faint);">Custom categories extend the standard Primary/Updates/Social/Promotions tabs. AI can suggest new categories based on your email patterns.</p>

    <!-- Existing categories -->
    {#if customCategories.length > 0}
      <div class="space-y-2 mb-4">
        {#each customCategories as cat}
          <div class="p-3 rounded-lg border" style="border-color: var(--iris-color-border);">
            <div class="flex items-center gap-3">
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2 mb-0.5">
                  <span class="text-sm font-medium" style="color: var(--iris-color-text);">{cat.name}</span>
                  {#if cat.is_ai_generated}
                    <span class="px-1.5 py-0.5 text-[10px] rounded-full font-medium"
                      style="background: color-mix(in srgb, var(--iris-color-primary) 15%, transparent); color: var(--iris-color-primary);">
                      AI
                    </span>
                  {:else}
                    <span class="px-1.5 py-0.5 text-[10px] rounded-full font-medium"
                      style="background: color-mix(in srgb, var(--iris-color-text-faint) 15%, transparent); color: var(--iris-color-text-faint);">
                      Manual
                    </span>
                  {/if}
                  {#if cat.status === 'suggested'}
                    <span class="px-1.5 py-0.5 text-[10px] rounded-full font-medium"
                      style="background: color-mix(in srgb, var(--iris-color-warning) 15%, transparent); color: var(--iris-color-warning);">
                      New
                    </span>
                  {/if}
                </div>
                {#if cat.description}
                  <p class="text-xs" style="color: var(--iris-color-text-faint);">{cat.description}</p>
                {/if}
                <span class="text-[10px]" style="color: var(--iris-color-text-faint);">{cat.email_count} email{cat.email_count !== 1 ? 's' : ''}</span>
              </div>
              <div class="flex items-center gap-1 shrink-0">
                {#if cat.status === 'suggested'}
                  <button class="settings-btn-primary px-2 py-1 text-xs font-medium rounded transition-colors"
                    style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
                    onclick={() => acceptCategory(cat.id)}>Accept</button>
                  <button class="settings-btn-secondary px-2 py-1 text-xs rounded border transition-colors"
                    style="border-color: var(--iris-color-border); color: var(--iris-color-text-muted);"
                    onclick={() => dismissCategory(cat.id)}>Dismiss</button>
                {:else}
                  <button class="settings-revoke-btn px-2 py-1 text-xs rounded transition-colors"
                    style="color: var(--iris-color-error);" onclick={() => deleteCustomCategory(cat.id)}>Delete</button>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      </div>
    {:else if !catShowNew}
      <p class="text-sm mb-4" style="color: var(--iris-color-text-faint);">No custom categories yet. Create one manually or let AI suggest categories.</p>
    {/if}

    <!-- New category form -->
    {#if catShowNew}
      <div class="p-3 rounded-lg border space-y-2 mb-4" style="border-color: var(--iris-color-border);">
        <FormInput bind:value={catNewName} placeholder="Category name (e.g., Receipts, Travel)" />
        <FormInput bind:value={catNewDescription} placeholder="Description (optional)" />
        <div class="flex gap-2">
          <button class="settings-btn-secondary px-3 py-1.5 text-xs rounded-lg border transition-colors"
            style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
            onclick={() => { catShowNew = false; catNewName = ''; catNewDescription = ''; }}>Cancel</button>
          <button class="settings-btn-primary px-3 py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-50"
            style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
            onclick={createCustomCategory} disabled={catSaving || !catNewName.trim()}>{catSaving ? 'Creating...' : 'Create Category'}</button>
        </div>
      </div>
    {/if}

    <div class="flex items-center gap-2">
      {#if !catShowNew}
        <button class="settings-btn-secondary px-4 py-2 text-sm rounded-lg border transition-colors"
          style="border-color: var(--iris-color-border); color: var(--iris-color-text);"
          onclick={() => (catShowNew = true)}>+ Add Category</button>
      {/if}
      <button class="settings-btn-primary px-4 py-2 text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
        style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
        onclick={analyzeCategories} disabled={catAnalyzing}>{catAnalyzing ? 'Analyzing...' : 'AI Suggest'}</button>
      {#if catAnalyzeMessage}
        <span class="text-xs" style="color: var(--iris-color-text-muted);">{catAnalyzeMessage}</span>
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

  .settings-revoke-btn:hover {
    filter: brightness(1.3);
  }

  .inline-select {
    font-family: var(--iris-font-family);
    outline: none;
  }

  .inline-select:focus {
    border-color: var(--iris-color-input-border-focus);
    box-shadow: 0 0 0 3px var(--iris-color-focus-ring);
  }
</style>
