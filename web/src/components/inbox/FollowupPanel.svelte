<script lang="ts">
  import { api, type FollowupReminder } from '../../lib/api';

  let { collapsed: initialCollapsed = false }: { collapsed?: boolean } = $props();

  let reminders = $state<FollowupReminder[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let scanning = $state(false);
  let error = $state('');
  let collapsed = $state(initialCollapsed);
  let snoozeTarget = $state<string | null>(null);
  let snoozeDate = $state('');

  async function loadReminders() {
    loading = true;
    error = '';
    try {
      const res = await api.followups.list();
      reminders = res.reminders;
      total = res.total;
    } catch {
      error = 'Failed to load reminders.';
    } finally {
      loading = false;
    }
  }

  async function scanForFollowups() {
    scanning = true;
    error = '';
    try {
      const res = await api.followups.scan();
      if (res.reminders_created > 0) {
        await loadReminders();
      }
    } catch (e: any) {
      if (e.message?.includes('503')) {
        error = 'Enable AI in Settings to scan.';
      } else {
        error = 'Scan failed. Try again.';
      }
    } finally {
      scanning = false;
    }
  }

  async function snooze(id: string) {
    if (!snoozeDate) return;
    try {
      await api.followups.snooze(id, snoozeDate);
      snoozeTarget = null;
      snoozeDate = '';
      await loadReminders();
    } catch {
      error = 'Failed to snooze.';
    }
  }

  async function dismiss(id: string) {
    try {
      await api.followups.dismiss(id);
      await loadReminders();
    } catch {
      error = 'Failed to dismiss.';
    }
  }

  async function markActed(id: string) {
    try {
      await api.followups.acted(id);
      await loadReminders();
    } catch {
      error = 'Failed to update.';
    }
  }

  function urgencyColor(urgency: string): string {
    switch (urgency) {
      case 'urgent': return 'var(--iris-color-error)';
      case 'high': return 'var(--iris-color-warning)';
      case 'normal': return 'var(--iris-color-primary)';
      case 'low': return 'var(--iris-color-text-faint)';
      default: return 'var(--iris-color-text-muted)';
    }
  }

  function formatDate(dateStr: string): string {
    try {
      const d = new Date(dateStr + 'T00:00:00');
      return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
    } catch {
      return dateStr;
    }
  }

  // Load on mount
  $effect(() => {
    loadReminders();
  });
</script>

<div
  class="border rounded-lg overflow-hidden"
  style="border-color: var(--iris-color-border); background: var(--iris-color-bg-elevated);"
>
  <!-- Header -->
  <button
    class="w-full px-4 py-3 flex items-center justify-between text-left hover:opacity-90 transition-opacity"
    onclick={() => (collapsed = !collapsed)}
  >
    <div class="flex items-center gap-2">
      <svg class="w-4 h-4" style="color: var(--iris-color-primary);" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
        <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z" />
      </svg>
      <span class="text-sm font-semibold" style="color: var(--iris-color-text);">Follow-up Reminders</span>
      {#if total > 0}
        <span
          class="px-1.5 py-0.5 text-[10px] font-bold rounded-full"
          style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
        >{total}</span>
      {/if}
    </div>
    <svg
      class="w-4 h-4 transition-transform"
      style="color: var(--iris-color-text-muted); transform: rotate({collapsed ? '0' : '180'}deg);"
      fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor"
    >
      <path stroke-linecap="round" stroke-linejoin="round" d="m19.5 8.25-7.5 7.5-7.5-7.5" />
    </svg>
  </button>

  {#if !collapsed}
    <div class="border-t" style="border-color: var(--iris-color-border);">
      <!-- Scan button -->
      <div class="px-4 py-2 flex justify-end">
        <button
          class="px-3 py-1 text-xs font-medium rounded-md hover:opacity-90 transition-opacity disabled:opacity-50"
          style="background: var(--iris-color-bg-surface); border: 1px solid var(--iris-color-border); color: var(--iris-color-text-muted);"
          onclick={scanForFollowups}
          disabled={scanning}
        >
          {scanning ? 'Scanning...' : 'Scan for follow-ups'}
        </button>
      </div>

      {#if error}
        <div class="px-4 pb-2">
          <p class="text-xs" style="color: var(--iris-color-error);">{error}</p>
        </div>
      {/if}

      {#if loading}
        <div class="px-4 py-6 text-center">
          <p class="text-xs" style="color: var(--iris-color-text-faint);">Loading...</p>
        </div>
      {:else if reminders.length === 0}
        <div class="px-4 py-6 text-center">
          <p class="text-xs" style="color: var(--iris-color-text-faint);">No pending follow-ups</p>
        </div>
      {:else}
        <div class="divide-y" style="border-color: var(--iris-color-border-subtle);">
          {#each reminders as reminder (reminder.id)}
            <div class="px-4 py-3 space-y-2">
              <!-- Subject + urgency -->
              <div class="flex items-start justify-between gap-2">
                <p class="text-sm font-medium truncate flex-1" style="color: var(--iris-color-text);">
                  {reminder.subject || '(no subject)'}
                </p>
                <span
                  class="px-1.5 py-0.5 text-[10px] font-semibold rounded uppercase shrink-0"
                  style="color: {urgencyColor(reminder.urgency)}; border: 1px solid {urgencyColor(reminder.urgency)};"
                >
                  {reminder.urgency}
                </span>
              </div>

              <!-- Reason -->
              <p class="text-xs leading-relaxed" style="color: var(--iris-color-text-muted);">
                {reminder.reason}
              </p>

              <!-- Suggested date -->
              <p class="text-[11px]" style="color: var(--iris-color-text-faint);">
                Follow up by {formatDate(reminder.suggested_date)}
                {#if reminder.status === 'snoozed'}
                  &middot; Snoozed until {formatDate(reminder.snoozed_until || '')}
                {/if}
              </p>

              <!-- Actions -->
              <div class="flex items-center gap-2 pt-1">
                {#if snoozeTarget === reminder.id}
                  <input
                    type="date"
                    bind:value={snoozeDate}
                    class="px-2 py-1 text-xs rounded"
                    style="background: var(--iris-color-bg-surface); border: 1px solid var(--iris-color-border); color: var(--iris-color-text);"
                  />
                  <button
                    class="px-2 py-1 text-xs rounded hover:opacity-80 transition-opacity"
                    style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
                    onclick={() => snooze(reminder.id)}
                    disabled={!snoozeDate}
                  >OK</button>
                  <button
                    class="px-2 py-1 text-xs rounded hover:opacity-80 transition-opacity"
                    style="color: var(--iris-color-text-muted);"
                    onclick={() => { snoozeTarget = null; snoozeDate = ''; }}
                  >Cancel</button>
                {:else}
                  <button
                    class="px-2 py-1 text-xs rounded hover:opacity-80 transition-opacity"
                    style="color: var(--iris-color-text-muted); border: 1px solid var(--iris-color-border-subtle);"
                    onclick={() => { snoozeTarget = reminder.id; }}
                    title="Snooze reminder"
                  >Snooze</button>
                  <button
                    class="px-2 py-1 text-xs rounded hover:opacity-80 transition-opacity"
                    style="color: var(--iris-color-text-faint); border: 1px solid var(--iris-color-border-subtle);"
                    onclick={() => dismiss(reminder.id)}
                    title="Dismiss reminder"
                  >Dismiss</button>
                  <button
                    class="px-2 py-1 text-xs rounded hover:opacity-80 transition-opacity"
                    style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
                    onclick={() => markActed(reminder.id)}
                    title="Mark as followed up"
                  >Follow Up</button>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>
