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
      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full trust-badge"
        class:trust-pass={allPass}
        class:trust-fail={anyFail}
        class:trust-partial={!allPass && !anyFail}
        title="SPF: {trust.spf || 'unknown'}, DKIM: {trust.dkim || 'unknown'}, DMARC: {trust.dmarc || 'unknown'}">
        {#if allPass}Verified{:else if anyFail}Unverified{:else}Partial{/if}
      </span>
    {/if}

    {#if trackingPixels.length > 0}
      <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full trust-badge trust-tracking"
        title="{trackingPixels.length} tracking pixel(s): {trackingPixels.map(t => t.domain).join(', ')}">
        {trackingPixels.length} tracker{trackingPixels.length > 1 ? 's' : ''} blocked
      </span>
    {/if}
  </div>
{/if}

<style>
  .trust-badge.trust-pass {
    background: color-mix(in srgb, var(--iris-color-success) 15%, transparent);
    color: var(--iris-color-success);
  }
  .trust-badge.trust-fail {
    background: color-mix(in srgb, var(--iris-color-error) 15%, transparent);
    color: var(--iris-color-error);
  }
  .trust-badge.trust-partial {
    background: color-mix(in srgb, var(--iris-color-warning) 15%, transparent);
    color: var(--iris-color-warning);
  }
  .trust-badge.trust-tracking {
    background: color-mix(in srgb, var(--iris-color-warning) 15%, transparent);
    color: var(--iris-color-warning);
  }
</style>
