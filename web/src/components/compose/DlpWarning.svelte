<script lang="ts">
  import { api } from '../../lib/api';

  interface DlpFinding {
    type: 'credit_card' | 'ssn' | 'api_key' | 'password' | 'private_key' | 'bank_account';
    match: string;
    location: 'subject' | 'body';
    line: number;
  }

  interface DlpScanResult {
    findings: DlpFinding[];
    risk_level: 'none' | 'low' | 'high';
    allow_send: boolean;
  }

  let {
    result,
    oncancel,
    onoverride,
  }: {
    result: DlpScanResult;
    oncancel: () => void;
    onoverride: (token: string) => void;
  } = $props();

  let holdProgress = $state(0);
  let holdInterval: ReturnType<typeof setInterval> | null = null;
  let overriding = $state(false);

  const HOLD_DURATION = 2000; // 2 seconds
  const TICK_INTERVAL = 50; // update every 50ms

  function startHold() {
    holdProgress = 0;
    holdInterval = setInterval(() => {
      holdProgress += (TICK_INTERVAL / HOLD_DURATION) * 100;
      if (holdProgress >= 100) {
        stopHold();
        performOverride();
      }
    }, TICK_INTERVAL);
  }

  function stopHold() {
    if (holdInterval) {
      clearInterval(holdInterval);
      holdInterval = null;
    }
    holdProgress = 0;
  }

  async function performOverride() {
    overriding = true;
    try {
      const res = await api.dlp.override();
      onoverride(res.token);
    } catch {
      // If override fails, just close
      oncancel();
    } finally {
      overriding = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      stopHold();
      oncancel();
    }
  }

  const typeLabels: Record<string, string> = {
    credit_card: 'Credit Card',
    ssn: 'Social Security Number',
    api_key: 'API Key',
    password: 'Password',
    private_key: 'Private Key',
    bank_account: 'Bank Account',
  };

  const isHighRisk = $derived(result.risk_level === 'high');
  const riskColor = $derived(isHighRisk ? 'var(--iris-color-error)' : 'var(--iris-color-warning)');
  const riskLabel = $derived(isHighRisk ? 'High Risk' : 'Low Risk');
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="fixed inset-0 z-[60] flex items-center justify-center"
  style="background: var(--iris-color-overlay);"
  role="alertdialog"
  aria-modal="true"
  aria-label="Sensitive data detected"
  tabindex="-1"
  onkeydown={handleKeydown}
>
  <div
    class="w-full max-w-lg mx-4 rounded-xl shadow-2xl overflow-hidden"
    style="background: var(--iris-color-bg-elevated); border: 1px solid {riskColor};"
  >
    <!-- Header -->
    <div
      class="flex items-center gap-3 px-5 py-4"
      style="border-bottom: 1px solid var(--iris-color-border); background: color-mix(in srgb, {riskColor} 8%, transparent);"
    >
      <!-- Shield icon -->
      <svg
        width="24" height="24" viewBox="0 0 24 24" fill="none"
        stroke={riskColor} stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
        <line x1="12" y1="8" x2="12" y2="12" />
        <line x1="12" y1="16" x2="12.01" y2="16" />
      </svg>
      <div>
        <h2 class="font-semibold text-base" style="color: var(--iris-color-text);">
          Sensitive Data Detected
        </h2>
        <span
          class="inline-block text-xs font-medium px-2 py-0.5 rounded-full mt-1"
          style="background: color-mix(in srgb, {riskColor} 15%, transparent); color: {riskColor};"
        >
          {riskLabel}
        </span>
      </div>
    </div>

    <!-- Findings list -->
    <div class="px-5 py-4 max-h-[300px] overflow-y-auto">
      <p class="text-xs mb-3" style="color: var(--iris-color-text-muted);">
        The following sensitive data was found in your email:
      </p>
      <ul class="space-y-2">
        {#each result.findings as finding}
          <li
            class="flex items-start gap-3 rounded-lg px-3 py-2.5"
            style="background: var(--iris-color-bg-surface); border: 1px solid var(--iris-color-border-subtle);"
          >
            <!-- Type icon -->
            <span class="flex-shrink-0 mt-0.5" style="color: {riskColor};">
              {#if finding.type === 'credit_card'}
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <rect x="1" y="4" width="22" height="16" rx="2" ry="2" />
                  <line x1="1" y1="10" x2="23" y2="10" />
                </svg>
              {:else if finding.type === 'ssn'}
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
                  <circle cx="12" cy="7" r="4" />
                  <line x1="18" y1="8" x2="23" y2="13" />
                  <line x1="23" y1="8" x2="18" y2="13" />
                </svg>
              {:else if finding.type === 'api_key'}
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.78 7.78 5.5 5.5 0 0 1 7.78-7.78zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4" />
                </svg>
              {:else if finding.type === 'password'}
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
                  <path d="M7 11V7a5 5 0 0 1 10 0v4" />
                </svg>
              {:else if finding.type === 'private_key'}
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                  <path d="M14 2v6h6" />
                  <path d="M12 18a2 2 0 1 0 0-4 2 2 0 0 0 0 4z" />
                  <path d="M12 14v-4" />
                </svg>
              {:else if finding.type === 'bank_account'}
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <line x1="3" y1="22" x2="21" y2="22" />
                  <line x1="6" y1="18" x2="6" y2="11" />
                  <line x1="10" y1="18" x2="10" y2="11" />
                  <line x1="14" y1="18" x2="14" y2="11" />
                  <line x1="18" y1="18" x2="18" y2="11" />
                  <polygon points="12 2 20 7 4 7" />
                </svg>
              {/if}
            </span>
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2">
                <span class="text-xs font-medium" style="color: var(--iris-color-text);">
                  {typeLabels[finding.type] || finding.type}
                </span>
                <span class="text-xs px-1.5 py-0.5 rounded" style="background: var(--iris-color-bg); color: var(--iris-color-text-faint);">
                  {finding.location} line {finding.line}
                </span>
              </div>
              <code class="text-xs mt-1 block truncate" style="color: var(--iris-color-text-muted); font-family: var(--iris-font-mono);">
                {finding.match}
              </code>
            </div>
          </li>
        {/each}
      </ul>
    </div>

    <!-- Footer -->
    <div
      class="flex items-center justify-end gap-3 px-5 py-4"
      style="border-top: 1px solid var(--iris-color-border);"
    >
      <button
        class="px-4 py-2 text-sm rounded-lg font-medium dlp-review-btn"
        style="color: var(--iris-color-text); background: var(--iris-color-bg-surface); border: 1px solid var(--iris-color-border);"
        onclick={oncancel}
      >
        Review Email
      </button>

      <!-- Hold-to-send button -->
      <button
        class="relative px-4 py-2 text-sm rounded-lg font-medium overflow-hidden dlp-override-btn"
        style="color: white; background: {riskColor};"
        onpointerdown={startHold}
        onpointerup={stopHold}
        onpointerleave={stopHold}
        disabled={overriding}
      >
        <!-- Progress fill -->
        {#if holdProgress > 0}
          <div
            class="absolute inset-0 opacity-30"
            style="background: white; width: {holdProgress}%; transition: width {TICK_INTERVAL}ms linear;"
          ></div>
        {/if}
        <span class="relative z-10">
          {#if overriding}
            Sending...
          {:else if holdProgress > 0}
            Hold to Send ({Math.ceil((100 - holdProgress) / 100 * 2)}s)
          {:else}
            Send Anyway
          {/if}
        </span>
      </button>
    </div>
  </div>
</div>

<style>
  .dlp-review-btn {
    transition: all var(--iris-transition-fast);
  }
  .dlp-review-btn:hover {
    background: var(--iris-color-bg);
    border-color: var(--iris-color-text-faint);
  }

  .dlp-override-btn {
    transition: all var(--iris-transition-fast);
    user-select: none;
    -webkit-user-select: none;
    touch-action: none;
  }
  .dlp-override-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }
  .dlp-override-btn:active:not(:disabled) {
    transform: scale(0.98);
  }
  .dlp-override-btn:disabled {
    opacity: 0.6;
  }
</style>
