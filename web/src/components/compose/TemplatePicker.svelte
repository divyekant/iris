<script lang="ts">
  import { api } from '../../lib/api';

  interface Template {
    id: string;
    name: string;
    subject: string | null;
    body_text: string;
    body_html: string | null;
    created_at: number;
    updated_at: number;
  }

  let {
    onpick,
    onclose,
  }: {
    onpick: (template: { subject: string; body_text: string }) => void;
    onclose: () => void;
  } = $props();

  let templates = $state<Template[]>([]);
  let loading = $state(true);

  $effect(() => {
    async function loadTemplates() {
      try {
        templates = await api.templates.list();
      } catch {
        templates = [];
      } finally {
        loading = false;
      }
    }
    loadTemplates();
  });

  function pickTemplate(t: Template) {
    onpick({ subject: t.subject || '', body_text: t.body_text });
    onclose();
  }

  function navigateToSettings() {
    onclose();
    window.location.hash = '#/settings';
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="template-picker-backdrop" onclick={onclose} onkeydown={(e) => e.key === 'Escape' && onclose()}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="template-picker" onclick={(e) => e.stopPropagation()}>
    <div class="template-picker-header">
      <span class="template-picker-title">Templates</span>
    </div>

    <div class="template-picker-list">
      {#if loading}
        <div class="template-picker-empty">Loading...</div>
      {:else if templates.length === 0}
        <div class="template-picker-empty">No templates yet</div>
      {:else}
        {#each templates as t}
          <button class="template-picker-item" onclick={() => pickTemplate(t)}>
            <svg class="template-picker-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"></path>
              <path d="M14 2v4a2 2 0 0 0 2 2h4"></path>
              <path d="M10 13H8"></path>
              <path d="M16 13h-2"></path>
              <path d="M10 17H8"></path>
              <path d="M16 17h-2"></path>
            </svg>
            <div class="template-picker-item-content">
              <span class="template-picker-item-name">{t.name}</span>
              {#if t.subject}
                <span class="template-picker-item-subject">{t.subject}</span>
              {/if}
            </div>
          </button>
        {/each}
      {/if}
    </div>

    <div class="template-picker-footer">
      <button class="template-picker-manage" onclick={navigateToSettings}>
        Manage Templates...
      </button>
    </div>
  </div>
</div>

<style>
  .template-picker-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
  }

  .template-picker {
    position: absolute;
    bottom: 60px;
    left: 50%;
    transform: translateX(-50%);
    min-width: 260px;
    max-width: 340px;
    border-radius: var(--iris-radius-lg, 12px);
    background: var(--iris-color-bg-elevated);
    border: 1px solid var(--iris-color-border);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .template-picker-header {
    padding: 8px 12px;
    border-bottom: 1px solid var(--iris-color-border-subtle);
  }

  .template-picker-title {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--iris-color-text-muted);
  }

  .template-picker-list {
    max-height: 240px;
    overflow-y: auto;
    padding: 4px 0;
  }

  .template-picker-empty {
    padding: 16px 12px;
    text-align: center;
    font-size: 13px;
    color: var(--iris-color-text-faint);
  }

  .template-picker-item {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    text-align: left;
    background: transparent;
    border: none;
    cursor: pointer;
    transition: background 120ms ease;
    color: var(--iris-color-text);
  }

  .template-picker-item:hover {
    background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent);
  }

  .template-picker-icon {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    margin-top: 2px;
    color: var(--iris-color-text-faint);
  }

  .template-picker-item-content {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .template-picker-item-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--iris-color-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .template-picker-item-subject {
    font-size: 11px;
    color: var(--iris-color-text-faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .template-picker-footer {
    padding: 6px 12px;
    border-top: 1px solid var(--iris-color-border-subtle);
  }

  .template-picker-manage {
    width: 100%;
    padding: 4px 0;
    text-align: left;
    background: transparent;
    border: none;
    cursor: pointer;
    font-size: 12px;
    color: var(--iris-color-primary);
    transition: opacity 120ms ease;
  }

  .template-picker-manage:hover {
    opacity: 0.8;
  }
</style>
