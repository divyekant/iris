<script lang="ts">
  import { wsClient } from '../../lib/ws';

  let syncing = $state(false);
  let syncMessage = $state('');

  $effect(() => {
    const offStatus = wsClient.on('SyncStatus', (event) => {
      syncing = true;
      syncMessage = event.data?.message || 'Syncing...';
    });

    const offComplete = wsClient.on('SyncComplete', () => {
      syncing = false;
      syncMessage = '';
    });

    return () => {
      offStatus();
      offComplete();
    };
  });
</script>

{#if syncing}
  <div class="px-4 py-2 bg-blue-50 dark:bg-blue-900/20 border-b border-blue-100 dark:border-blue-800 flex items-center gap-2">
    <div class="w-4 h-4 border-2 border-blue-300 border-t-blue-600 rounded-full animate-spin"></div>
    <span class="text-xs text-blue-600 dark:text-blue-400">{syncMessage}</span>
  </div>
{/if}
