<script lang="ts">
  interface Props {
    variant?: 'error' | 'warning' | 'success' | 'info' | 'neutral' | 'brand';
    text: string;
    dot?: boolean;
    size?: 'sm' | 'md';
  }

  let {
    variant = 'neutral',
    text,
    dot = false,
    size = 'md',
  }: Props = $props();

  const colorMap: Record<string, string> = {
    error: 'var(--iris-color-error)',
    warning: 'var(--iris-color-warning)',
    success: 'var(--iris-color-success)',
    info: 'var(--iris-color-info)',
    neutral: 'var(--iris-color-text-muted)',
    brand: 'var(--iris-color-primary)',
  };

  let color = $derived(colorMap[variant] ?? colorMap.neutral);
</script>

<span
  class="badge"
  class:badge-sm={size === 'sm'}
  style="--badge-color: {color};"
>
  {#if dot}
    <span class="badge-dot"></span>
  {/if}
  {text}
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-family: var(--iris-font-family);
    font-size: 12px;
    font-weight: 500;
    line-height: 1;
    color: var(--badge-color);
    background: color-mix(in srgb, var(--badge-color) 12%, transparent);
    border-radius: 9999px;
    padding: 4px 10px;
    white-space: nowrap;
  }

  .badge-sm {
    font-size: 11px;
    padding: 2px 8px;
  }

  .badge-dot {
    width: 6px;
    height: 6px;
    border-radius: 9999px;
    background: var(--badge-color);
    flex-shrink: 0;
  }
</style>
