<script lang="ts">
  let { trust, trackingPixels = [], impersonationRisk = null }: {
    trust: { spf?: string; dkim?: string; dmarc?: string };
    trackingPixels?: { url: string; domain: string }[];
    impersonationRisk?: { lookalike_of: string; risk_level: string } | null;
  } = $props();

  const allPass = $derived(trust?.spf === 'pass' && trust?.dkim === 'pass' && trust?.dmarc === 'pass');
  const anyFail = $derived(trust?.spf === 'fail' || trust?.dkim === 'fail' || trust?.dmarc === 'fail');
  const hasTrust = $derived(trust?.spf || trust?.dkim || trust?.dmarc);
  const isHighRisk = $derived(impersonationRisk?.risk_level === 'high');
</script>

{#if impersonationRisk}
  <div class="flex items-center gap-2 px-3 py-2 rounded-lg mb-2 impersonation-banner"
    class:impersonation-high={isHighRisk}
    class:impersonation-medium={!isHighRisk}>
    <svg class="w-4 h-4 flex-shrink-0" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
      <path fill-rule="evenodd" d="M8.485 2.495c.673-1.167 2.357-1.167 3.03 0l6.28 10.875c.673 1.167-.168 2.625-1.516 2.625H3.72c-1.347 0-2.189-1.458-1.515-2.625L8.485 2.495zM10 5a.75.75 0 01.75.75v3.5a.75.75 0 01-1.5 0v-3.5A.75.75 0 0110 5zm0 8a1 1 0 100-2 1 1 0 000 2z" clip-rule="evenodd" />
    </svg>
    <span class="text-xs font-medium">
      Possible impersonation — looks like <strong>{impersonationRisk.lookalike_of}</strong>
    </span>
  </div>
{/if}

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
  .impersonation-banner {
    line-height: 1.4;
  }
  .impersonation-banner.impersonation-high {
    background: color-mix(in srgb, var(--iris-color-error) 15%, transparent);
    color: var(--iris-color-error);
    border: 1px solid color-mix(in srgb, var(--iris-color-error) 30%, transparent);
  }
  .impersonation-banner.impersonation-medium {
    background: color-mix(in srgb, var(--iris-color-warning) 15%, transparent);
    color: var(--iris-color-warning);
    border: 1px solid color-mix(in srgb, var(--iris-color-warning) 30%, transparent);
  }
</style>
