<script lang="ts">
  type ReplyOption = { tone: string; subject: string; body: string };

  let {
    options,
    loading,
    error,
    onpick,
    onclose,
  }: {
    options: ReplyOption[];
    loading: boolean;
    error: string;
    onpick: (option: ReplyOption) => void;
    onclose: () => void;
  } = $props();

  let selectedTone = $state<string | null>(null);

  const toneStyles: Record<string, { bg: string; text: string; label: string }> = {
    formal: { bg: 'color-mix(in srgb, var(--iris-color-primary) 13%, transparent)', text: 'var(--iris-color-primary)', label: 'Formal' },
    casual: { bg: 'color-mix(in srgb, var(--iris-color-info) 13%, transparent)', text: 'var(--iris-color-info)', label: 'Casual' },
    brief: { bg: 'color-mix(in srgb, var(--iris-color-success) 13%, transparent)', text: 'var(--iris-color-success)', label: 'Brief' },
  };

  function getToneStyle(tone: string) {
    return toneStyles[tone] || toneStyles.formal;
  }

  function handlePick(option: ReplyOption) {
    selectedTone = option.tone;
    onpick(option);
  }
</script>

<div class="multi-reply-picker rounded-xl p-4 space-y-3" style="background: var(--iris-color-bg-surface); border: 1px solid var(--iris-color-border);">
  <div class="flex items-center justify-between">
    <h4 class="text-sm font-medium" style="color: var(--iris-color-text);">AI Reply Options</h4>
    <button
      class="text-xs px-2 py-0.5 rounded"
      style="color: var(--iris-color-text-faint);"
      onclick={onclose}
    >&times; Close</button>
  </div>

  {#if loading}
    <div class="grid grid-cols-1 sm:grid-cols-3 gap-3">
      {#each ['formal', 'casual', 'brief'] as tone}
        {@const style = getToneStyle(tone)}
        <div class="rounded-lg p-3 space-y-2 animate-pulse" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);">
          <span class="inline-block px-2 py-0.5 rounded-full text-[10px] font-semibold" style="background: {style.bg}; color: {style.text};">
            {style.label}
          </span>
          <div class="space-y-1.5">
            <div class="h-3 rounded" style="background: var(--iris-color-border); width: 60%;"></div>
            <div class="h-3 rounded" style="background: var(--iris-color-border); width: 90%;"></div>
            <div class="h-3 rounded" style="background: var(--iris-color-border); width: 75%;"></div>
          </div>
        </div>
      {/each}
    </div>
  {:else if error}
    <p class="text-xs py-2" style="color: var(--iris-color-error);">{error}</p>
  {:else}
    <div class="grid grid-cols-1 sm:grid-cols-3 gap-3">
      {#each options as option (option.tone)}
        {@const style = getToneStyle(option.tone)}
        <div
          class="rounded-lg p-3 space-y-2 transition-all cursor-pointer multi-reply-card"
          style="background: var(--iris-color-bg-elevated); border: 2px solid {selectedTone === option.tone ? style.text : 'var(--iris-color-border)'};"
          role="button"
          tabindex="0"
          onclick={() => handlePick(option)}
          onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handlePick(option); } }}
        >
          <span
            class="inline-block px-2 py-0.5 rounded-full text-[10px] font-semibold"
            style="background: {style.bg}; color: {style.text};"
          >
            {style.label}
          </span>
          <p class="text-xs font-medium truncate" style="color: var(--iris-color-text-muted);">{option.subject}</p>
          <p class="text-xs leading-relaxed line-clamp-6" style="color: var(--iris-color-text);">{option.body}</p>
          <button
            class="w-full mt-2 px-3 py-1.5 text-xs rounded-lg font-medium transition-colors multi-reply-use-btn"
            style="background: {selectedTone === option.tone ? style.text : 'var(--iris-color-bg-surface)'}; color: {selectedTone === option.tone ? 'var(--iris-color-bg)' : style.text}; border: 1px solid {style.text}40;"
            onclick={(e) => { e.stopPropagation(); handlePick(option); }}
          >
            {selectedTone === option.tone ? 'Selected' : 'Use This Draft'}
          </button>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .multi-reply-card:hover {
    background: var(--iris-color-bg-hover);
  }
  .multi-reply-use-btn:hover {
    background: var(--iris-color-primary-hover);
  }
  .line-clamp-6 {
    display: -webkit-box;
    -webkit-line-clamp: 6;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
</style>
