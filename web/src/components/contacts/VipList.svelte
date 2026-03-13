<script lang="ts">
  import { Crown, RefreshCw, Loader2 } from 'lucide-svelte';
  import { api, type VipContact } from '../../lib/api';
  import ErrorBanner from '../ErrorBanner.svelte';

  let contacts: VipContact[] = $state([]);
  let loading = $state(true);
  let computing = $state(false);
  let error = $state('');

  async function loadVips() {
    loading = true;
    error = '';
    try {
      const res = await api.vip.list();
      contacts = res.vip_contacts;
    } catch (e: any) {
      error = e.message || 'Failed to load VIP contacts';
    } finally {
      loading = false;
    }
  }

  async function recompute() {
    computing = true;
    error = '';
    try {
      await api.vip.compute();
      await loadVips();
    } catch (e: any) {
      error = e.message || 'Failed to recompute scores';
    } finally {
      computing = false;
    }
  }

  async function toggleVip(contact: VipContact) {
    const newState = !contact.is_manual;
    try {
      await api.vip.setVip(contact.email, newState);
      await loadVips();
    } catch (e: any) {
      error = e.message || 'Failed to toggle VIP status';
    }
  }

  function getInitials(name: string | null, email: string): string {
    if (name) {
      const parts = name.trim().split(/\s+/);
      if (parts.length >= 2) {
        return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
      }
      return name[0].toUpperCase();
    }
    return email[0].toUpperCase();
  }

  $effect(() => {
    loadVips();
  });
</script>

<div class="vip-list">
  <div class="vip-header">
    <h3 class="vip-title">VIP Contacts</h3>
    <button
      class="recompute-btn"
      onclick={recompute}
      disabled={computing}
      title="Recompute VIP scores"
    >
      {#if computing}
        <Loader2 size={16} class="animate-spin" />
        Computing...
      {:else}
        <RefreshCw size={16} />
        Recompute Scores
      {/if}
    </button>
  </div>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  {#if loading}
    <div class="vip-loading">
      <Loader2 size={20} class="animate-spin" />
      <span>Loading VIP contacts...</span>
    </div>
  {:else if contacts.length === 0}
    <div class="vip-empty">
      <Crown size={32} />
      <p>No VIP contacts yet</p>
      <p class="vip-empty-hint">Click "Recompute Scores" to detect VIP contacts from your email history.</p>
    </div>
  {:else}
    <div class="vip-contacts">
      {#each contacts as contact (contact.email)}
        <div class="vip-contact-row">
          <div class="vip-avatar" title={contact.display_name || contact.email}>
            {getInitials(contact.display_name, contact.email)}
          </div>

          <div class="vip-contact-info">
            <div class="vip-contact-name">
              {contact.display_name || contact.email}
            </div>
            {#if contact.display_name}
              <div class="vip-contact-email">{contact.email}</div>
            {/if}
          </div>

          <div class="vip-score-bar-container" title={`Score: ${(contact.vip_score * 100).toFixed(0)}%`}>
            <div class="vip-score-bar">
              <div
                class="vip-score-fill"
                style="width: {Math.round(contact.vip_score * 100)}%"
              ></div>
            </div>
            <span class="vip-score-label">{(contact.vip_score * 100).toFixed(0)}%</span>
          </div>

          <button
            class="vip-star-toggle"
            class:vip-star-active={contact.is_manual}
            onclick={() => toggleVip(contact)}
            title={contact.is_manual ? 'Remove manual VIP' : 'Set as manual VIP'}
          >
            <Crown size={18} />
          </button>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .vip-list {
    display: flex;
    flex-direction: column;
    gap: calc(var(--iris-spacing-base) * 4);
  }

  .vip-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: calc(var(--iris-spacing-base) * 3);
  }

  .vip-title {
    font-size: 1rem;
    font-weight: 600;
    color: var(--iris-color-text);
    margin: 0;
  }

  .recompute-btn {
    display: inline-flex;
    align-items: center;
    gap: calc(var(--iris-spacing-base) * 2);
    padding: calc(var(--iris-spacing-base) * 2) calc(var(--iris-spacing-base) * 3);
    border-radius: var(--iris-radius-md);
    border: 1px solid var(--iris-color-border);
    background: var(--iris-color-bg-surface);
    color: var(--iris-color-text-muted);
    font-size: 0.8125rem;
    cursor: pointer;
    transition: all var(--iris-transition-fast);
  }

  .recompute-btn:hover:not(:disabled) {
    border-color: var(--iris-color-primary);
    color: var(--iris-color-primary);
  }

  .recompute-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .vip-loading {
    display: flex;
    align-items: center;
    gap: calc(var(--iris-spacing-base) * 2);
    color: var(--iris-color-text-muted);
    font-size: 0.875rem;
    padding: calc(var(--iris-spacing-base) * 4);
  }

  .vip-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: calc(var(--iris-spacing-base) * 2);
    padding: calc(var(--iris-spacing-base) * 8);
    color: var(--iris-color-text-faint);
    text-align: center;
  }

  .vip-empty p {
    margin: 0;
    font-size: 0.875rem;
  }

  .vip-empty-hint {
    font-size: 0.8125rem;
    color: var(--iris-color-text-faint);
  }

  .vip-contacts {
    display: flex;
    flex-direction: column;
  }

  .vip-contact-row {
    display: flex;
    align-items: center;
    gap: calc(var(--iris-spacing-base) * 3);
    padding: calc(var(--iris-spacing-base) * 3);
    border-bottom: 1px solid var(--iris-color-border-subtle);
    transition: background var(--iris-transition-fast);
  }

  .vip-contact-row:hover {
    background: var(--iris-color-bg-surface);
  }

  .vip-avatar {
    flex-shrink: 0;
    width: 36px;
    height: 36px;
    border-radius: var(--iris-radius-full);
    background: color-mix(in srgb, var(--iris-color-primary) 15%, transparent);
    color: var(--iris-color-primary);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 0.8125rem;
    font-weight: 600;
  }

  .vip-contact-info {
    flex: 1;
    min-width: 0;
  }

  .vip-contact-name {
    font-size: 0.875rem;
    font-weight: 500;
    color: var(--iris-color-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .vip-contact-email {
    font-size: 0.75rem;
    color: var(--iris-color-text-faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .vip-score-bar-container {
    display: flex;
    align-items: center;
    gap: calc(var(--iris-spacing-base) * 2);
    flex-shrink: 0;
    width: 120px;
  }

  .vip-score-bar {
    flex: 1;
    height: 6px;
    border-radius: var(--iris-radius-full);
    background: var(--iris-color-bg-surface);
    overflow: hidden;
  }

  .vip-score-fill {
    height: 100%;
    border-radius: var(--iris-radius-full);
    background: var(--iris-color-primary);
    transition: width var(--iris-transition-normal);
  }

  .vip-score-label {
    font-size: 0.75rem;
    color: var(--iris-color-text-muted);
    font-variant-numeric: tabular-nums;
    min-width: 32px;
    text-align: right;
  }

  .vip-star-toggle {
    flex-shrink: 0;
    padding: calc(var(--iris-spacing-base) * 1);
    border: none;
    background: none;
    color: var(--iris-color-text-faint);
    cursor: pointer;
    border-radius: var(--iris-radius-sm);
    transition: all var(--iris-transition-fast);
  }

  .vip-star-toggle:hover {
    color: var(--iris-color-primary);
    background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent);
  }

  .vip-star-active {
    color: var(--iris-color-primary);
  }
</style>
