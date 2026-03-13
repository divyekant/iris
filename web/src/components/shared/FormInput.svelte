<script lang="ts">
  interface Props {
    label?: string;
    value?: string;
    placeholder?: string;
    type?: 'text' | 'email' | 'password' | 'number';
    error?: string;
    disabled?: boolean;
    name?: string;
  }

  let {
    label,
    value = $bindable(''),
    placeholder = '',
    type = 'text',
    error,
    disabled = false,
    name,
  }: Props = $props();

  let focused = $state(false);
</script>

<div class="form-input-wrapper">
  {#if label}
    <label class="form-input-label" for={name}>{label}</label>
  {/if}
  <input
    class="form-input"
    class:has-error={!!error}
    class:is-focused={focused}
    {type}
    {name}
    id={name}
    {placeholder}
    {disabled}
    bind:value
    onfocus={() => focused = true}
    onblur={() => focused = false}
  />
  {#if error}
    <p class="form-input-error">{error}</p>
  {/if}
</div>

<style>
  .form-input-wrapper {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .form-input-label {
    font-family: var(--iris-font-family);
    font-size: 13px;
    font-weight: 500;
    color: var(--iris-color-text-muted);
  }

  .form-input {
    font-family: var(--iris-font-family);
    font-size: 13px;
    color: var(--iris-color-text);
    background: var(--iris-color-input-bg);
    border: 1px solid var(--iris-color-input-border);
    border-radius: var(--iris-radius-md);
    padding: 8px 12px;
    outline: none;
    transition: border-color var(--iris-transition-fast),
                box-shadow var(--iris-transition-fast);
  }

  .form-input::placeholder {
    color: var(--iris-color-text-faint);
  }

  .form-input.is-focused {
    border-color: var(--iris-color-input-border-focus);
    box-shadow: 0 0 0 3px var(--iris-color-focus-ring);
  }

  .form-input.has-error {
    border-color: var(--iris-color-input-border-error);
  }

  .form-input.has-error.is-focused {
    border-color: var(--iris-color-input-border-error);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--iris-color-error) 30%, transparent);
  }

  .form-input:disabled {
    opacity: 0.5;
    background: var(--iris-color-bg-disabled);
    cursor: not-allowed;
  }

  .form-input-error {
    font-family: var(--iris-font-family);
    font-size: 12px;
    color: var(--iris-color-error);
    margin: 0;
  }
</style>
