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
  <div class="px-4 py-2 border-b flex items-center gap-2" style="background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent); border-color: var(--iris-color-border);">
    <div class="w-4 h-4 border-2 rounded-full animate-spin" style="border-color: color-mix(in srgb, var(--iris-color-primary) 30%, transparent); border-top-color: var(--iris-color-primary);"></div>
    <span class="text-xs" style="color: var(--iris-color-primary);">{syncMessage}</span>
  </div>
{/if}
