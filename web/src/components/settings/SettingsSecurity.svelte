<script lang="ts">
  import { api } from '../../lib/api';
  import FormInput from '../shared/FormInput.svelte';
  import FormSelect from '../shared/FormSelect.svelte';

  let { accounts = $bindable([]) }: { accounts: any[] } = $props();

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

  const permissionOptions = [
    { value: 'read_only', label: 'Read Only' },
    { value: 'draft_only', label: 'Draft Only' },
    { value: 'send_with_approval', label: 'Send w/ Approval' },
    { value: 'autonomous', label: 'Autonomous' },
  ];

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

  // Load data on mount
  $effect(() => {
    async function loadData() {
      await loadBlockedSenders();
      await loadApiKeys();
      await loadAuditLog();
    }
    loadData();
  });
</script>

<div class="space-y-8">
  <!-- Blocked Senders section -->
  <section>
    <h3 class="text-sm font-semibold uppercase tracking-wider mb-4" style="color: var(--iris-color-text-muted);">Blocked Senders</h3>
    <p class="text-xs mb-4" style="color: var(--iris-color-text-faint);">Emails from blocked senders are automatically moved to Spam.</p>

    <div class="flex gap-2 mb-4">
      <div class="flex-1">
        <FormInput
          type="email"
          bind:value={newBlockEmail}
          placeholder="Block a sender manually (email address)"
        />
      </div>
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
      <div class="flex-1">
        <FormInput
          bind:value={newKeyName}
          placeholder="Key name (e.g., Claude agent)"
        />
      </div>
      <FormSelect
        bind:value={newKeyPermission}
        options={permissionOptions}
      />
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

<style>
  .settings-btn-primary:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .settings-revoke-btn:hover {
    filter: brightness(1.3);
  }
</style>
