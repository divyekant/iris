<script lang="ts">
  import { ShieldAlert, AlertTriangle, Info, ChevronDown, ChevronUp, X } from 'lucide-svelte';

  interface SocialEngineeringTactic {
    type: string;
    evidence: string;
    confidence: number;
  }

  interface SocialEngineeringResult {
    risk_level: 'none' | 'low' | 'medium' | 'high' | 'critical';
    tactics: SocialEngineeringTactic[];
    summary: string;
  }

  let { result }: { result: SocialEngineeringResult | null } = $props();

  let dismissed = $state(false);
  let detailsOpen = $state(false);

  const show = $derived(
    result &&
    result.risk_level !== 'none' &&
    !dismissed
  );

  const isCriticalOrHigh = $derived(
    result?.risk_level === 'critical' || result?.risk_level === 'high'
  );

  const isMedium = $derived(result?.risk_level === 'medium');
  const isLow = $derived(result?.risk_level === 'low');

  function formatTacticType(type: string): string {
    return type
      .split('_')
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(' ');
  }

  function confidenceLabel(confidence: number): string {
    if (confidence >= 0.8) return 'High';
    if (confidence >= 0.5) return 'Medium';
    return 'Low';
  }
</script>

{#if show}
  <div
    class="se-banner"
    class:se-critical={isCriticalOrHigh}
    class:se-medium={isMedium}
    class:se-low={isLow}
    role="alert"
  >
    <div class="se-banner-header">
      <div class="se-banner-icon">
        {#if isCriticalOrHigh}
          <ShieldAlert size={18} />
        {:else if isMedium}
          <AlertTriangle size={18} />
        {:else}
          <Info size={18} />
        {/if}
      </div>

      <div class="se-banner-content">
        <div class="se-banner-title">
          {#if isCriticalOrHigh}
            Social Engineering Alert
          {:else if isMedium}
            Suspicious Content Detected
          {:else}
            Minor Concern
          {/if}
          <span class="se-risk-badge">
            {result?.risk_level}
          </span>
        </div>

        <p class="se-banner-summary">{result?.summary}</p>
      </div>

      <div class="se-banner-actions">
        {#if result && result.tactics.length > 0}
          <button
            class="se-details-btn"
            onclick={() => (detailsOpen = !detailsOpen)}
            aria-expanded={detailsOpen}
          >
            {detailsOpen ? 'Hide' : 'Details'}
            {#if detailsOpen}
              <ChevronUp size={14} />
            {:else}
              <ChevronDown size={14} />
            {/if}
          </button>
        {/if}

        <button
          class="se-dismiss-btn"
          onclick={() => (dismissed = true)}
          title="Dismiss"
          aria-label="Dismiss warning"
        >
          <X size={14} />
        </button>
      </div>
    </div>

    {#if detailsOpen && result && result.tactics.length > 0}
      <div class="se-details">
        {#each result.tactics as tactic}
          <div class="se-tactic">
            <div class="se-tactic-header">
              <span class="se-tactic-type">{formatTacticType(tactic.type)}</span>
              <span class="se-tactic-confidence">
                {confidenceLabel(tactic.confidence)} confidence ({Math.round(tactic.confidence * 100)}%)
              </span>
            </div>
            {#if tactic.evidence}
              <blockquote class="se-tactic-evidence">"{tactic.evidence}"</blockquote>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .se-banner {
    border-radius: var(--iris-radius-md);
    padding: calc(var(--iris-spacing-base) * 3);
    margin-bottom: calc(var(--iris-spacing-base) * 3);
    transition: var(--iris-transition-fast);
  }

  .se-banner.se-critical {
    background: color-mix(in srgb, var(--iris-color-error) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--iris-color-error) 30%, transparent);
  }
  .se-banner.se-critical .se-banner-icon,
  .se-banner.se-critical .se-banner-title {
    color: var(--iris-color-error);
  }

  .se-banner.se-medium {
    background: color-mix(in srgb, var(--iris-color-warning) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--iris-color-warning) 30%, transparent);
  }
  .se-banner.se-medium .se-banner-icon,
  .se-banner.se-medium .se-banner-title {
    color: var(--iris-color-warning);
  }

  .se-banner.se-low {
    background: color-mix(in srgb, var(--iris-color-info) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--iris-color-info) 20%, transparent);
  }
  .se-banner.se-low .se-banner-icon,
  .se-banner.se-low .se-banner-title {
    color: var(--iris-color-info);
  }

  .se-banner-header {
    display: flex;
    align-items: flex-start;
    gap: calc(var(--iris-spacing-base) * 3);
  }

  .se-banner-icon {
    flex-shrink: 0;
    margin-top: 1px;
  }

  .se-banner-content {
    flex: 1;
    min-width: 0;
  }

  .se-banner-title {
    font-size: 13px;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: calc(var(--iris-spacing-base) * 2);
  }

  .se-risk-badge {
    font-size: 10px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 1px 6px;
    border-radius: var(--iris-radius-sm, 4px);
    background: color-mix(in srgb, currentColor 15%, transparent);
  }

  .se-banner-summary {
    font-size: 12px;
    color: var(--iris-color-text-muted);
    margin-top: calc(var(--iris-spacing-base) * 1);
    line-height: 1.5;
  }

  .se-banner-actions {
    display: flex;
    align-items: center;
    gap: calc(var(--iris-spacing-base) * 1);
    flex-shrink: 0;
  }

  .se-details-btn {
    display: flex;
    align-items: center;
    gap: 2px;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: var(--iris-radius-sm, 4px);
    color: var(--iris-color-text-muted);
    background: transparent;
    border: none;
    cursor: pointer;
    transition: var(--iris-transition-fast);
  }
  .se-details-btn:hover {
    background: color-mix(in srgb, var(--iris-color-text-muted) 10%, transparent);
  }

  .se-dismiss-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: var(--iris-radius-sm, 4px);
    color: var(--iris-color-text-faint);
    background: transparent;
    border: none;
    cursor: pointer;
    transition: var(--iris-transition-fast);
  }
  .se-dismiss-btn:hover {
    color: var(--iris-color-text-muted);
    background: color-mix(in srgb, var(--iris-color-text-muted) 10%, transparent);
  }

  .se-details {
    margin-top: calc(var(--iris-spacing-base) * 3);
    padding-top: calc(var(--iris-spacing-base) * 3);
    border-top: 1px solid color-mix(in srgb, var(--iris-color-border) 50%, transparent);
    display: flex;
    flex-direction: column;
    gap: calc(var(--iris-spacing-base) * 3);
  }

  .se-tactic {
    padding-left: calc(var(--iris-spacing-base) * 3);
  }

  .se-tactic-header {
    display: flex;
    align-items: center;
    gap: calc(var(--iris-spacing-base) * 2);
    margin-bottom: calc(var(--iris-spacing-base) * 1);
  }

  .se-tactic-type {
    font-size: 12px;
    font-weight: 600;
    color: var(--iris-color-text);
  }

  .se-tactic-confidence {
    font-size: 10px;
    color: var(--iris-color-text-faint);
  }

  .se-tactic-evidence {
    font-size: 12px;
    color: var(--iris-color-text-muted);
    font-style: italic;
    margin: 0;
    padding-left: calc(var(--iris-spacing-base) * 3);
    border-left: 2px solid color-mix(in srgb, var(--iris-color-border) 70%, transparent);
    line-height: 1.5;
  }
</style>
