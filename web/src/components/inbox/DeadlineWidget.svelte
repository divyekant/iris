<script lang="ts">
  import { api, type Deadline } from '../../lib/api';

  let deadlines = $state<Deadline[]>([]);
  let expanded = $state(false);
  let loading = $state(true);

  async function loadDeadlines() {
    try {
      loading = true;
      const res = await api.deadlines.list(7);
      deadlines = res.deadlines;
    } catch {
      deadlines = [];
    } finally {
      loading = false;
    }
  }

  async function toggleComplete(deadline: Deadline) {
    try {
      if (!deadline.completed) {
        await api.deadlines.complete(deadline.id);
        deadline.completed = true;
        // Refresh to update count
        await loadDeadlines();
      }
    } catch {
      // silent
    }
  }

  function formatDate(dateStr: string): string {
    const date = new Date(dateStr + 'T00:00:00');
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const deadline = new Date(dateStr + 'T00:00:00');
    deadline.setHours(0, 0, 0, 0);
    const diffDays = Math.round((deadline.getTime() - today.getTime()) / (1000 * 60 * 60 * 24));

    if (diffDays < 0) return `${Math.abs(diffDays)}d overdue`;
    if (diffDays === 0) return 'Today';
    if (diffDays === 1) return 'Tomorrow';
    return date.toLocaleDateString([], { month: 'short', day: 'numeric' });
  }

  function dateColor(dateStr: string): string {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const deadline = new Date(dateStr + 'T00:00:00');
    deadline.setHours(0, 0, 0, 0);
    const diff = deadline.getTime() - today.getTime();
    if (diff < 0) return 'var(--iris-color-error)';
    if (diff === 0) return 'var(--iris-color-warning)';
    return 'var(--iris-color-info)';
  }

  $effect(() => {
    loadDeadlines();
  });
</script>

{#if !loading && deadlines.length > 0}
  <div class="deadline-widget">
    <button
      class="flex items-center gap-2 px-3 py-1.5 rounded-full text-xs font-medium transition-colors widget-toggle"
      onclick={() => (expanded = !expanded)}
    >
      <svg xmlns="http://www.w3.org/2000/svg" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="color: var(--iris-color-primary);">
        <rect width="18" height="18" x="3" y="4" rx="2" ry="2"></rect>
        <line x1="16" x2="16" y1="2" y2="6"></line>
        <line x1="8" x2="8" y1="2" y2="6"></line>
        <line x1="3" x2="21" y1="10" y2="10"></line>
      </svg>
      <span style="color: var(--iris-color-text);">Deadlines</span>
      <span class="count-badge">{deadlines.length}</span>
      <span style="color: var(--iris-color-text-muted);">{expanded ? '\u25BE' : '\u25B8'}</span>
    </button>

    {#if expanded}
      <div class="widget-panel mt-1 rounded-lg p-2 space-y-1">
        {#each deadlines as deadline (deadline.id)}
          <div class="flex items-center gap-2 px-2 py-1.5 rounded widget-item">
            <button
              class="flex-shrink-0 w-3.5 h-3.5 rounded border flex items-center justify-center"
              style="border-color: {dateColor(deadline.deadline_date)};"
              onclick={() => toggleComplete(deadline)}
              title="Mark complete"
            >
              {#if deadline.completed}
                <svg xmlns="http://www.w3.org/2000/svg" width="8" height="8" viewBox="0 0 24 24" fill="none" stroke="{dateColor(deadline.deadline_date)}" stroke-width="3" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="20 6 9 17 4 12"></polyline>
                </svg>
              {/if}
            </button>
            <span class="text-xs truncate flex-1" style="color: var(--iris-color-text);">
              {deadline.description}
            </span>
            <span class="text-[10px] flex-shrink-0 font-medium" style="color: {dateColor(deadline.deadline_date)};">
              {formatDate(deadline.deadline_date)}
            </span>
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .widget-toggle {
    background: var(--iris-color-bg-surface);
    border: 1px solid var(--iris-color-border-subtle);
  }
  .widget-toggle:hover {
    background: var(--iris-color-bg-elevated);
  }
  .count-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 4px;
    border-radius: var(--iris-radius-full);
    font-size: 10px;
    font-weight: 600;
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .widget-panel {
    border: 1px solid var(--iris-color-border-subtle);
    background: var(--iris-color-bg-elevated);
    max-height: 240px;
    overflow-y: auto;
  }
  .widget-item {
    transition: background var(--iris-transition-fast);
  }
  .widget-item:hover {
    background: var(--iris-color-bg-surface);
  }
</style>
