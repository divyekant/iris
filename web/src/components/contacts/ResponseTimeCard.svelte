<script lang="ts">
  import { api } from '../../lib/api';

  let { email }: { email: string } = $props();

  let data = $state<{
    email: string;
    their_avg_reply_hours: number | null;
    your_avg_reply_hours: number | null;
    their_reply_count: number;
    your_reply_count: number;
    fastest_reply_hours: number | null;
    slowest_reply_hours: number | null;
    total_exchanges: number;
  } | null>(null);
  let loading = $state(true);
  let error = $state('');

  function formatHours(hours: number | null): string {
    if (hours === null) return '--';
    if (hours < 1) {
      const mins = Math.round(hours * 60);
      return `${mins}m`;
    }
    if (hours < 24) {
      return `${hours.toFixed(1)}h`;
    }
    const days = hours / 24;
    return `${days.toFixed(1)}d`;
  }

  function speedColor(hours: number | null): string {
    if (hours === null) return 'var(--iris-color-text-faint)';
    if (hours < 2) return 'var(--iris-color-success)';
    if (hours <= 24) return 'var(--iris-color-primary)';
    return 'var(--iris-color-error)';
  }

  function speedLabel(hours: number | null): string {
    if (hours === null) return '';
    if (hours < 2) return 'Fast';
    if (hours <= 24) return 'Medium';
    return 'Slow';
  }

  $effect(() => {
    if (email) {
      loading = true;
      error = '';
      api.contacts
        .responseTimes(email)
        .then((res) => {
          data = res;
        })
        .catch((e) => {
          error = e.message || 'Failed to load';
        })
        .finally(() => {
          loading = false;
        });
    }
  });
</script>

<div class="response-time-card">
  <h4 class="card-title">Response Times</h4>

  {#if loading}
    <div class="loading-state">
      <div class="spinner"></div>
      <span>Loading...</span>
    </div>
  {:else if error}
    <p class="error-text">{error}</p>
  {:else if data}
    <div class="stats-grid">
      <div class="stat-block">
        <span class="stat-label">Their avg reply</span>
        <span class="stat-value" style="color: {speedColor(data.their_avg_reply_hours)};">
          {formatHours(data.their_avg_reply_hours)}
        </span>
        {#if data.their_avg_reply_hours !== null}
          <span class="stat-badge" style="color: {speedColor(data.their_avg_reply_hours)};">
            {speedLabel(data.their_avg_reply_hours)}
          </span>
        {/if}
        <span class="stat-detail">{data.their_reply_count} replies</span>
      </div>

      <div class="stat-block">
        <span class="stat-label">Your avg reply</span>
        <span class="stat-value" style="color: {speedColor(data.your_avg_reply_hours)};">
          {formatHours(data.your_avg_reply_hours)}
        </span>
        {#if data.your_avg_reply_hours !== null}
          <span class="stat-badge" style="color: {speedColor(data.your_avg_reply_hours)};">
            {speedLabel(data.your_avg_reply_hours)}
          </span>
        {/if}
        <span class="stat-detail">{data.your_reply_count} replies</span>
      </div>
    </div>

    <div class="detail-row">
      <div class="detail-item">
        <span class="detail-label">Fastest</span>
        <span class="detail-value">{formatHours(data.fastest_reply_hours)}</span>
      </div>
      <div class="detail-item">
        <span class="detail-label">Slowest</span>
        <span class="detail-value">{formatHours(data.slowest_reply_hours)}</span>
      </div>
      <div class="detail-item">
        <span class="detail-label">Exchanges</span>
        <span class="detail-value">{data.total_exchanges}</span>
      </div>
    </div>
  {/if}
</div>

<style>
  .response-time-card {
    background: var(--iris-color-bg-elevated);
    border: 1px solid var(--iris-color-border-subtle);
    border-radius: var(--iris-radius-lg);
    padding: 16px;
  }

  .card-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--iris-color-text-muted);
    margin: 0 0 12px 0;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .loading-state {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--iris-color-text-faint);
    font-size: 12px;
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid var(--iris-color-border);
    border-top-color: var(--iris-color-primary);
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .error-text {
    color: var(--iris-color-text-faint);
    font-size: 12px;
    margin: 0;
  }

  .stats-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
    margin-bottom: 12px;
  }

  .stat-block {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .stat-label {
    font-size: 11px;
    color: var(--iris-color-text-faint);
  }

  .stat-value {
    font-size: 20px;
    font-weight: 700;
    font-family: var(--iris-font-mono);
    line-height: 1.2;
  }

  .stat-badge {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .stat-detail {
    font-size: 10px;
    color: var(--iris-color-text-faint);
  }

  .detail-row {
    display: flex;
    gap: 16px;
    padding-top: 12px;
    border-top: 1px solid var(--iris-color-border-subtle);
  }

  .detail-item {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .detail-label {
    font-size: 10px;
    color: var(--iris-color-text-faint);
  }

  .detail-value {
    font-size: 12px;
    font-weight: 600;
    color: var(--iris-color-text-muted);
    font-family: var(--iris-font-mono);
  }
</style>
