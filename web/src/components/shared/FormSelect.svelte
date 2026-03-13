<script lang="ts">
  interface SelectOption {
    value: string;
    label: string;
  }

  interface Props {
    label?: string;
    value?: string;
    options?: SelectOption[];
    disabled?: boolean;
    error?: string;
  }

  let {
    label,
    value = $bindable(''),
    options = [],
    disabled = false,
    error,
  }: Props = $props();

  let focused = $state(false);
</script>

<div class="form-select-wrapper">
  {#if label}
    <label class="form-select-label">{label}</label>
  {/if}
  <div class="form-select-container">
    <select
      class="form-select"
      class:has-error={!!error}
      class:is-focused={focused}
      {disabled}
      bind:value
      onfocus={() => focused = true}
      onblur={() => focused = false}
    >
      {#each options as opt}
        <option value={opt.value}>{opt.label}</option>
      {/each}
    </select>
    <svg class="form-select-chevron" width="16" height="16" viewBox="0 0 16 16" fill="none">
      <path d="M4 6L8 10L12 6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
    </svg>
  </div>
  {#if error}
    <p class="form-select-error">{error}</p>
  {/if}
</div>

<style>
  .form-select-wrapper {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .form-select-label {
    font-family: var(--iris-font-family);
    font-size: 13px;
    font-weight: 500;
    color: var(--iris-color-text-muted);
  }

  .form-select-container {
    position: relative;
    display: flex;
    align-items: center;
  }

  .form-select {
    width: 100%;
    font-family: var(--iris-font-family);
    font-size: 13px;
    color: var(--iris-color-text);
    background: var(--iris-color-input-bg);
    border: 1px solid var(--iris-color-input-border);
    border-radius: var(--iris-radius-md);
    padding: 8px 36px 8px 12px;
    outline: none;
    appearance: none;
    -webkit-appearance: none;
    cursor: pointer;
    transition: border-color var(--iris-transition-fast),
                box-shadow var(--iris-transition-fast);
  }

  .form-select.is-focused {
    border-color: var(--iris-color-input-border-focus);
    box-shadow: 0 0 0 3px var(--iris-color-focus-ring);
  }

  .form-select.has-error {
    border-color: var(--iris-color-input-border-error);
  }

  .form-select.has-error.is-focused {
    border-color: var(--iris-color-input-border-error);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--iris-color-error) 30%, transparent);
  }

  .form-select:disabled {
    opacity: 0.5;
    background: var(--iris-color-bg-disabled);
    cursor: not-allowed;
  }

  .form-select-chevron {
    position: absolute;
    right: 12px;
    pointer-events: none;
    color: var(--iris-color-text-faint);
  }

  .form-select-error {
    font-family: var(--iris-font-family);
    font-size: 12px;
    color: var(--iris-color-error);
    margin: 0;
  }
</style>
