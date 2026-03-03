<script lang="ts">
  import { push } from 'svelte-spa-router';

  let { onchatToggle, chatOpen = false }: {
    onchatToggle?: () => void;
    chatOpen?: boolean;
  } = $props();

  function openSearch() {
    push('/search');
  }
</script>

<header class="h-14 border-b border-gray-200 dark:border-gray-700 flex items-center px-4 gap-4">
  <div class="flex-1">
    <input
      type="text"
      placeholder="Search emails..."
      readonly
      class="w-full max-w-md px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-800 text-sm placeholder-gray-400 cursor-pointer hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors"
      onfocus={openSearch}
      onclick={openSearch}
    />
  </div>
  <button
    class="px-3 py-1.5 text-sm rounded-lg transition-colors {chatOpen
      ? 'bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'
      : 'text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-800'}"
    onclick={onchatToggle}
    title="AI Chat"
  >
    AI Chat
  </button>
</header>
