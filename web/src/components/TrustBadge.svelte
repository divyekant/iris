<script lang="ts">
  let { trust, trackingPixels = [] }: {
    trust: { spf?: string; dkim?: string; dmarc?: string };
    trackingPixels?: { url: string; domain: string }[];
  } = $props();

  const allPass = $derived(trust?.spf === 'pass' && trust?.dkim === 'pass' && trust?.dmarc === 'pass');
  const anyFail = $derived(trust?.spf === 'fail' || trust?.dkim === 'fail' || trust?.dmarc === 'fail');
  const hasTrust = $derived(trust?.spf || trust?.dkim || trust?.dmarc);
</script>

{#if hasTrust || trackingPixels.length > 0}
  <div class="flex items-center gap-2 text-xs">
    {#if hasTrust}
      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full
        {allPass ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400'
         : anyFail ? 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400'
         : 'bg-yellow-100 text-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-400'}"
        title="SPF: {trust.spf || 'unknown'}, DKIM: {trust.dkim || 'unknown'}, DMARC: {trust.dmarc || 'unknown'}">
        {#if allPass}Verified{:else if anyFail}Unverified{:else}Partial{/if}
      </span>
    {/if}

    {#if trackingPixels.length > 0}
      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400"
        title="{trackingPixels.length} tracking pixel(s): {trackingPixels.map(t => t.domain).join(', ')}">
        {trackingPixels.length} tracker{trackingPixels.length > 1 ? 's' : ''} blocked
      </span>
    {/if}
  </div>
{/if}
