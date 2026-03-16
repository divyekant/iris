<script lang="ts">
  import type { Snippet } from 'svelte';
  import { irisFade } from '$lib/transitions';

  let { text = '', position = 'top', children }: {
    text: string;
    position?: 'top' | 'bottom' | 'left' | 'right';
    children: Snippet;
  } = $props();

  let visible = $state(false);
  const isVertical = $derived(position === 'top' || position === 'bottom');
</script>

<span
  onmouseenter={() => visible = true}
  onmouseleave={() => visible = false}
  onfocus={() => visible = true}
  onblur={() => visible = false}
  class="inline-flex relative"
  role="group"
>
  {@render children()}
  {#if visible && text}
    <span
      transition:irisFade
      role="tooltip"
      class="absolute z-50 px-2 py-1 text-xs rounded whitespace-nowrap pointer-events-none"
      class:bottom-full={position === 'top'}
      class:top-full={position === 'bottom'}
      class:right-full={position === 'left'}
      class:left-full={position === 'right'}
      class:mb-1={position === 'top'}
      class:mt-1={position === 'bottom'}
      class:mr-1={position === 'left'}
      class:ml-1={position === 'right'}
      style="
        background: var(--iris-color-bg-elevated);
        color: var(--iris-color-text);
        border: 1px solid var(--iris-color-border);
        {isVertical ? 'left: 50%; transform: translateX(-50%);' : 'top: 50%; transform: translateY(-50%);'}
      "
    >
      {text}
    </span>
  {/if}
</span>
