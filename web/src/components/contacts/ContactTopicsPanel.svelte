<script lang="ts">
  import { api } from '../../lib/api';
  import { X } from 'lucide-svelte';

  let { email, name = null, onclose }: {
    email: string;
    name?: string | null;
    onclose: () => void;
  } = $props();

  let loading = $state(true);
  let error = $state('');
  let topics = $state<{ topic: string; count: number }[]>([]);
  let totalEmails = $state(0);
  let cached = $state(false);

  const pillColors = [
    '#d4af37', // gold (primary)
    '#2563EB', // blue (info)
    '#16A34A', // green (success)
    '#DC2626', // red (error)
    '#CA8A04', // amber (warning)
  ];

  function getInitials(displayName: string | null, addr: string): string {
    const src = displayName || addr;
    const parts = src.split(/[\s@.]+/).filter(Boolean);
    if (parts.length >= 2) return (parts[0][0] + parts[1][0]).toUpperCase();
    return src.slice(0, 2).toUpperCase();
  }

  function pillColor(index: number): string {
    return pillColors[index % pillColors.length];
  }

  async function loadTopics() {
    loading = true;
    error = '';
    try {
      const res = await api.contacts.topics(email);
      topics = res.topics;
      totalEmails = res.total_emails;
      cached = res.cached;
    } catch (e: any) {
      error = e.message || 'Failed to load topics';
    } finally {
      loading = false;
    }
  }

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) onclose();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose();
  }

  $effect(() => {
    if (email) loadTopics();
  });
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed inset-0 z-50 flex items-start justify-center pt-24"
  style="background: var(--iris-color-overlay);"
  onclick={handleBackdropClick}
>
  <div
    class="w-full max-w-sm rounded-xl shadow-lg panel"
    style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);"
  >
    <!-- Header -->
    <div class="flex items-center gap-3 p-4" style="border-bottom: 1px solid var(--iris-color-border);">
      <div
        class="w-9 h-9 rounded-full flex items-center justify-center text-sm font-semibold flex-shrink-0"
        style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
      >
        {getInitials(name, email)}
      </div>
      <div class="flex-1 min-w-0">
        {#if name}
          <div class="text-sm font-medium truncate" style="color: var(--iris-color-text);">{name}</div>
        {/if}
        <div class="text-xs truncate" style="color: var(--iris-color-text-muted);">{email}</div>
        {#if !loading && !error}
          <div class="text-[10px] flex items-center gap-1.5" style="color: var(--iris-color-text-faint);">
            {totalEmails} email{totalEmails === 1 ? '' : 's'}
            {#if cached}
              <span class="px-1 py-0.5 rounded" style="background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent);">Cached</span>
            {/if}
          </div>
        {/if}
      </div>
      <button
        class="p-1 rounded transition-colors close-btn"
        onclick={onclose}
        title="Close"
      >
        <X size={16} />
      </button>
    </div>

    <!-- Content -->
    <div class="p-4">
      {#if loading}
        <div class="flex items-center gap-2 py-6 justify-center">
          <div class="w-4 h-4 rounded-full animate-spin" style="border: 2px solid var(--iris-color-border); border-top-color: var(--iris-color-primary);"></div>
          <span class="text-xs" style="color: var(--iris-color-text-faint);">Analyzing topics...</span>
        </div>
      {:else if error}
        <div class="text-center py-6">
          <p class="text-xs mb-2" style="color: var(--iris-color-error);">{error}</p>
          <button
            class="text-xs px-3 py-1 rounded-md transition-colors retry-btn"
            onclick={loadTopics}
          >Retry</button>
        </div>
      {:else if topics.length === 0}
        <div class="text-center py-6">
          <p class="text-sm" style="color: var(--iris-color-text-faint);">No topics identified yet</p>
          <p class="text-xs mt-1" style="color: var(--iris-color-text-faint);">Topics appear after exchanging a few emails.</p>
        </div>
      {:else}
        <p class="text-xs font-medium mb-3" style="color: var(--iris-color-text-muted);">Key Topics</p>
        <div class="flex flex-wrap gap-2">
          {#each topics as topicItem, i}
            {@const color = pillColor(i)}
            <span
              class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium topic-pill"
              style="background: color-mix(in srgb, {color} 12%, transparent); color: {color}; border: 1px solid color-mix(in srgb, {color} 20%, transparent);"
            >
              {topicItem.topic}
              <span
                class="text-[10px] px-1 py-0.5 rounded-full font-semibold"
                style="background: color-mix(in srgb, {color} 15%, transparent);"
              >{topicItem.count}</span>
            </span>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .panel {
    animation: panel-in 150ms ease;
  }
  @keyframes panel-in {
    from { opacity: 0; transform: translateY(-8px); }
    to { opacity: 1; transform: translateY(0); }
  }
  .close-btn {
    color: var(--iris-color-text-faint);
  }
  .close-btn:hover {
    color: var(--iris-color-text);
    background: var(--iris-color-bg-surface);
  }
  .retry-btn {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .retry-btn:hover {
    filter: brightness(1.1);
  }
  .topic-pill {
    transition: all var(--iris-transition-fast);
  }
  .topic-pill:hover {
    filter: brightness(1.15);
  }
</style>
