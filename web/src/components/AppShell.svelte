<script lang="ts">
  import Sidebar from './Sidebar.svelte';
  import Header from './Header.svelte';
  import ChatPanel from './ChatPanel.svelte';
  import type { Snippet } from 'svelte';

  let { children }: { children: Snippet } = $props();
  let chatOpen = $state(false);
</script>

<div class="flex h-screen bg-white dark:bg-gray-900 text-gray-900 dark:text-gray-100">
  <Sidebar />
  <div class="flex flex-col flex-1 overflow-hidden">
    <Header onchatToggle={() => (chatOpen = !chatOpen)} chatOpen={chatOpen} />
    <div class="flex flex-1 overflow-hidden">
      <main class="flex-1 overflow-auto">
        {@render children()}
      </main>
      <ChatPanel open={chatOpen} onclose={() => (chatOpen = false)} />
    </div>
  </div>
</div>
