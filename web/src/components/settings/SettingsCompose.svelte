<script lang="ts">
  import { api } from '../../lib/api';

  let { accounts = $bindable([]) }: { accounts: any[] } = $props();

  // Signatures
  type SignatureItem = { id: string; account_id: string; name: string; body_text: string; body_html: string; is_default: boolean; created_at: number };
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

  // Templates functions
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

  // Signatures functions
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

  // Load data on mount
  $effect(() => {
    async function loadData() {
      if (accounts.length > 0 && !sigAccountId) {
        sigAccountId = accounts[0].id;
        await loadSignatures();
      }
      if (accounts.length > 0 && !aliasAccountId) {
        aliasAccountId = accounts[0].id;
      }
      await loadTemplates();
      await loadAliases();
    }
    loadData();
  });
</script>

<div class="space-y-8">
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

  .settings-edit-btn:hover {
    filter: brightness(1.2);
  }

  .settings-input::placeholder {
    color: var(--iris-color-text-faint);
  }
</style>
