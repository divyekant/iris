<script lang="ts">
  import MessageRow from './MessageRow.svelte';

  let { messages, onclick }: { messages: any[]; onclick: (id: string) => void } = $props();
</script>

{#if messages.length === 0}
  <div class="text-center py-16 text-gray-400 dark:text-gray-500">
    <p class="text-lg mb-2">No messages yet</p>
    <p class="text-sm">Add an email account to get started.</p>
  </div>
{:else}
  <div class="divide-y divide-gray-100 dark:divide-gray-800">
    {#each messages as message (message.id)}
      <MessageRow {message} {onclick} />
    {/each}
  </div>
{/if}
