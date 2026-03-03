<script lang="ts">
  import { push, location } from 'svelte-spa-router';

  const navItems = [
    { path: '/', label: 'Inbox', icon: '\u{1F4E5}' },
    { path: '/setup', label: 'Add Account', icon: '\u{2795}' },
    { path: '/settings', label: 'Settings', icon: '\u{2699}\u{FE0F}' },
  ];
</script>

<aside class="w-56 border-r border-gray-200 dark:border-gray-700 flex flex-col">
  <div class="p-4 border-b border-gray-200 dark:border-gray-700">
    <h1 class="text-xl font-bold">Iris</h1>
  </div>
  <nav class="flex-1 p-2 space-y-1">
    {#each navItems as item}
      <button
        class="w-full text-left px-3 py-2 rounded-lg text-sm transition-colors
               {$location === item.path ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 font-medium' : 'hover:bg-gray-100 dark:hover:bg-gray-800'}"
        onclick={() => push(item.path)}
      >
        <span class="mr-2">{item.icon}</span>
        {item.label}
      </button>
    {/each}
  </nav>
</aside>
