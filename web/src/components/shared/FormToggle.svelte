<script lang="ts">
  interface Props {
    label?: string;
    checked?: boolean;
    disabled?: boolean;
    description?: string;
  }

  let {
    label,
    checked = $bindable(false),
    disabled = false,
    description,
  }: Props = $props();
</script>

<label class="form-toggle" class:is-disabled={disabled}>
  <div class="form-toggle-text">
    {#if label}
      <span class="form-toggle-label">{label}</span>
    {/if}
    {#if description}
      <span class="form-toggle-description">{description}</span>
    {/if}
  </div>
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    class="form-toggle-track"
    class:is-on={checked}
    {disabled}
    onclick={() => { if (!disabled) checked = !checked; }}
  >
    <span class="form-toggle-thumb" class:is-on={checked}></span>
  </button>
</label>

<style>
  .form-toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    cursor: pointer;
  }

  .form-toggle.is-disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .form-toggle-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .form-toggle-label {
    font-family: var(--iris-font-family);
    font-size: 13px;
    font-weight: 500;
    color: var(--iris-color-text);
  }

  .form-toggle-description {
    font-family: var(--iris-font-family);
    font-size: 12px;
    color: var(--iris-color-text-muted);
  }

  .form-toggle-track {
    position: relative;
    width: 44px;
    height: 24px;
    border-radius: 12px;
    border: none;
    padding: 0;
    cursor: inherit;
    background: var(--iris-color-input-border);
    transition: background var(--iris-transition-fast);
    flex-shrink: 0;
  }

  .form-toggle-track.is-on {
    background: var(--iris-color-primary);
  }

  .form-toggle-thumb {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 18px;
    height: 18px;
    border-radius: 9999px;
    background: #ffffff;
    transition: transform var(--iris-transition-fast);
    pointer-events: none;
  }

  .form-toggle-thumb.is-on {
    transform: translateX(20px);
  }
</style>
