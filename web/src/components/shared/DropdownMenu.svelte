<!-- web/src/components/shared/DropdownMenu.svelte -->
<script lang="ts">
  import { irisScale } from '$lib/transitions';

  interface MenuItem {
    label: string;
    icon?: string;
    shortcut?: string;
    onClick: () => void;
    dividerAfter?: boolean;
    disabled?: boolean;
  }

  let { items = [], triggerLabel = '', triggerIcon = '' }: {
    items: MenuItem[];
    triggerLabel?: string;
    triggerIcon?: string;
  } = $props();

  let open = $state(false);
  let focusedIndex = $state(-1);
  let menuEl: HTMLElement;

  const enabledItems = $derived(items.filter(i => !i.disabled));

  function toggle() { open = !open; focusedIndex = -1; }
  function close() { open = false; focusedIndex = -1; }

  function handleKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === 'Escape') { close(); e.preventDefault(); }
    else if (e.key === 'ArrowDown') { focusedIndex = Math.min(focusedIndex + 1, enabledItems.length - 1); e.preventDefault(); }
    else if (e.key === 'ArrowUp') { focusedIndex = Math.max(focusedIndex - 1, 0); e.preventDefault(); }
    else if (e.key === 'Enter' && focusedIndex >= 0) { enabledItems[focusedIndex]?.onClick(); close(); e.preventDefault(); }
  }

  $effect(() => {
    if (!open) return;
    function handleOutsideClick(e: MouseEvent) {
      if (menuEl && !menuEl.contains(e.target as Node)) close();
    }
    window.addEventListener('click', handleOutsideClick);
    return () => window.removeEventListener('click', handleOutsideClick);
  });
</script>


<div class="relative inline-block" bind:this={menuEl} onkeydown={handleKeydown}>
  <button
    onclick={toggle}
    class="flex items-center gap-1 px-2 py-1 rounded text-sm transition-colors"
    style="color: var(--iris-color-text-muted); background: transparent;"
    onmouseenter={(e) => (e.currentTarget as HTMLElement).style.background = 'var(--iris-color-bg-hover)'}
    onmouseleave={(e) => (e.currentTarget as HTMLElement).style.background = 'transparent'}
  >
    {#if triggerIcon}<span>{triggerIcon}</span>{/if}
    {#if triggerLabel}<span>{triggerLabel}</span>{/if}
    <span class="text-xs">&#x25BE;</span>
  </button>

  {#if open}
    <div
      transition:irisScale
      class="absolute right-0 mt-1 min-w-[160px] rounded-lg shadow-lg z-50 py-1"
      style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);"
    >
      {#each enabledItems as item, i}
        <button
          onclick={() => { item.onClick(); close(); }}
          class="w-full text-left px-3 py-1.5 text-sm flex items-center justify-between gap-4 transition-colors"
          style="color: {focusedIndex === i ? 'var(--iris-color-text)' : 'var(--iris-color-text-muted)'}; background: {focusedIndex === i ? 'var(--iris-color-bg-hover)' : 'transparent'};"
          onmouseenter={() => focusedIndex = i}
        >
          <span class="flex items-center gap-2">
            {#if item.icon}<span>{item.icon}</span>{/if}
            {item.label}
          </span>
          {#if item.shortcut}
            <span class="text-xs" style="color: var(--iris-color-text-faint);">{item.shortcut}</span>
          {/if}
        </button>
        {#if item.dividerAfter}
          <hr class="my-1" style="border-color: var(--iris-color-border);" />
        {/if}
      {/each}
    </div>
  {/if}
</div>
