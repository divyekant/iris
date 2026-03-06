<script lang="ts">
  interface Props { widths?: number[]; }
  let { widths = [140, 320, 240] }: Props = $props();
</script>

<div class="flex items-center gap-3 px-6 h-[72px]">
  <div class="w-2 h-2 rounded-full" style="background: var(--iris-color-bg-surface);"></div>
  <div class="flex flex-col gap-2 flex-1">
    {#each widths as w, i}
      <div
        class="rounded animate-pulse"
        style="background: var(--iris-color-bg-surface); width: {w}px; height: {i < 2 ? 12 : 10}px;"
      ></div>
    {/each}
  </div>
</div>
