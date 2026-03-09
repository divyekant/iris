<script lang="ts">
  import MessageRow from './MessageRow.svelte';

  let { messages, onclick, selectedIds = $bindable(new Set<string>()), onaction, focusedIndex = -1 }: {
    messages: any[];
    onclick: (id: string) => void;
    selectedIds?: Set<string>;
    onaction?: (id: string, action: string) => void;
    focusedIndex?: number;
  } = $props();

  function handleSelect(id: string, checked: boolean) {
    const next = new Set(selectedIds);
    if (checked) {
      next.add(id);
    } else {
      next.delete(id);
    }
    selectedIds = next;
  }
</script>

{#if messages.length === 0}
  <div class="text-center py-16" style="color: var(--iris-color-text-faint);">
    <p class="text-lg mb-2">No messages yet</p>
    <p class="text-sm">Add an email account to get started.</p>
  </div>
{:else}
  <div>
    {#each messages as message, i (message.id)}
      <MessageRow {message} {onclick} selected={selectedIds.has(message.id)} focused={i === focusedIndex} onselect={handleSelect} {onaction} />
    {/each}
  </div>
{/if}
