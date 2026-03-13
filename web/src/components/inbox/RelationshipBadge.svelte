<script lang="ts">
  let { score = 0 }: { score: number } = $props();

  const strength = $derived(
    score > 0.7 ? 'strong' : score >= 0.4 ? 'medium' : 'weak'
  );
</script>

{#if strength !== 'weak'}
  <span
    class="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded rel-badge"
    class:rel-strong={strength === 'strong'}
    class:rel-medium={strength === 'medium'}
    title="Relationship strength: {(score * 100).toFixed(0)}%"
  >
    <!-- Person icon -->
    <svg
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
      {#if strength === 'strong'}
        <circle cx="12" cy="7" r="4" fill="currentColor" />
      {:else}
        <circle cx="12" cy="7" r="4" />
      {/if}
    </svg>
    <span class="text-[10px] font-medium">{(score * 100).toFixed(0)}%</span>
  </span>
{/if}

<style>
  .rel-badge {
    font-family: var(--iris-font-family);
    border-radius: var(--iris-radius-sm);
    transition: opacity var(--iris-transition-fast);
    line-height: 1;
  }
  .rel-strong {
    background: color-mix(in srgb, var(--iris-color-primary) 15%, transparent);
    color: var(--iris-color-primary);
  }
  .rel-medium {
    background: color-mix(in srgb, var(--iris-color-text-muted) 10%, transparent);
    color: var(--iris-color-text-muted);
  }
</style>
