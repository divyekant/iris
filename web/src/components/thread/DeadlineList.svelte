<script lang="ts">
  import { api, type Deadline } from '../../lib/api';

  let { threadId }: { threadId: string } = $props();
  let deadlines = $state<Deadline[]>([]);
  let loading = $state(true);
  let error = $state('');

  async function loadDeadlines() {
    try {
      loading = true;
      error = '';
      const res = await api.deadlines.forThread(threadId);
      deadlines = res.deadlines;
    } catch (e: any) {
      error = e.message || 'Failed to load deadlines';
    } finally {
      loading = false;
    }
  }

  async function toggleComplete(deadline: Deadline) {
    try {
      if (!deadline.completed) {
        await api.deadlines.complete(deadline.id);
        deadline.completed = true;
      }
    } catch {
      // Revert on failure handled by reactivity
    }
  }

  async function removeDeadline(id: string) {
    try {
      await api.deadlines.delete(id);
      deadlines = deadlines.filter((d) => d.id !== id);
    } catch {
      // silent
    }
  }

  function formatDate(dateStr: string): string {
    const date = new Date(dateStr + 'T00:00:00');
    return date.toLocaleDateString([], { month: 'short', day: 'numeric', year: 'numeric' });
  }

  function getDateStatus(dateStr: string, completed: boolean): 'overdue' | 'today' | 'upcoming' | 'completed' {
    if (completed) return 'completed';
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const deadline = new Date(dateStr + 'T00:00:00');
    deadline.setHours(0, 0, 0, 0);
    const diff = deadline.getTime() - today.getTime();
    if (diff < 0) return 'overdue';
    if (diff === 0) return 'today';
    return 'upcoming';
  }

  function statusColor(status: string): string {
    switch (status) {
      case 'overdue': return 'var(--iris-color-error)';
      case 'today': return 'var(--iris-color-warning)';
      case 'upcoming': return 'var(--iris-color-info)';
      case 'completed': return 'var(--iris-color-text-faint)';
      default: return 'var(--iris-color-text)';
    }
  }

  function statusLabel(status: string): string {
    switch (status) {
      case 'overdue': return 'Overdue';
      case 'today': return 'Due today';
      case 'upcoming': return 'Upcoming';
      case 'completed': return 'Done';
      default: return '';
    }
  }

  $effect(() => {
    if (threadId) loadDeadlines();
  });
</script>

{#if loading}
  <div class="py-2 text-xs" style="color: var(--iris-color-text-faint);">Loading deadlines...</div>
{:else if error}
  <div class="py-2 text-xs" style="color: var(--iris-color-error);">{error}</div>
{:else if deadlines.length > 0}
  <div class="deadline-list rounded-lg p-3 space-y-2">
    <div class="flex items-center gap-2 mb-2">
      <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="color: var(--iris-color-primary);">
        <rect width="18" height="18" x="3" y="4" rx="2" ry="2"></rect>
        <line x1="16" x2="16" y1="2" y2="6"></line>
        <line x1="8" x2="8" y1="2" y2="6"></line>
        <line x1="3" x2="21" y1="10" y2="10"></line>
      </svg>
      <span class="text-xs font-medium" style="color: var(--iris-color-text-muted);">
        Deadlines ({deadlines.length})
      </span>
    </div>

    {#each deadlines as deadline (deadline.id)}
      {@const status = getDateStatus(deadline.deadline_date, deadline.completed)}
      <div class="deadline-item flex items-start gap-2 p-2 rounded" style="border-left: 3px solid {statusColor(status)};">
        <button
          class="mt-0.5 flex-shrink-0 w-4 h-4 rounded border flex items-center justify-center transition-colors"
          style="border-color: {statusColor(status)}; background: {deadline.completed ? statusColor(status) : 'transparent'};"
          onclick={() => toggleComplete(deadline)}
          title={deadline.completed ? 'Completed' : 'Mark complete'}
        >
          {#if deadline.completed}
            <svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="20 6 9 17 4 12"></polyline>
            </svg>
          {/if}
        </button>

        <div class="flex-1 min-w-0">
          <p class="text-xs {deadline.completed ? 'line-through' : ''}" style="color: {deadline.completed ? 'var(--iris-color-text-faint)' : 'var(--iris-color-text)'};">
            {deadline.description}
          </p>
          <div class="flex items-center gap-2 mt-1">
            <span class="text-[10px] font-medium" style="color: {statusColor(status)};">
              {statusLabel(status)} - {formatDate(deadline.deadline_date)}
            </span>
            {#if deadline.deadline_source}
              <span class="text-[10px] italic" style="color: var(--iris-color-text-faint);">
                "{deadline.deadline_source}"
              </span>
            {/if}
          </div>
        </div>

        <button
          class="flex-shrink-0 p-1 rounded transition-colors hover:bg-[var(--iris-color-bg-surface)]"
          onclick={() => removeDeadline(deadline.id)}
          title="Remove deadline"
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--iris-color-text-faint)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 6h18"></path>
            <path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"></path>
            <path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"></path>
          </svg>
        </button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .deadline-list {
    border: 1px solid var(--iris-color-border-subtle);
    background: var(--iris-color-bg-elevated);
  }
  .deadline-item {
    background: var(--iris-color-bg-surface);
  }
</style>
