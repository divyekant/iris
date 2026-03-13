<script lang="ts">
  import { Crown } from 'lucide-svelte';

  let { score = 0, size = 14 }: {
    score?: number;
    size?: number;
  } = $props();

  let scoreText = $derived(`VIP score: ${(score * 100).toFixed(0)}%`);
</script>

<span
  class="inline-flex items-center flex-shrink-0 vip-badge"
  title={scoreText}
  role="img"
  aria-label={scoreText}
>
  <Crown size={size} />
</span>

<style>
  .vip-badge {
    color: var(--iris-color-primary);
    transition: opacity var(--iris-transition-fast);
  }
  .vip-badge:hover {
    opacity: 0.8;
  }
</style>
