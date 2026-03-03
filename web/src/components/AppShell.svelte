<script lang="ts">
  import Sidebar from './Sidebar.svelte';
  import Header from './Header.svelte';
  import type { Snippet } from 'svelte';

  let { children }: { children: Snippet } = $props();
</script>

<div class="flex h-screen bg-white dark:bg-gray-900 text-gray-900 dark:text-gray-100">
  <Sidebar />
  <div class="flex flex-col flex-1 overflow-hidden">
    <Header />
    <main class="flex-1 overflow-auto">
      {@render children()}
    </main>
  </div>
</div>
