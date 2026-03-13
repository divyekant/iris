<script lang="ts">
  let { intent }: { intent?: string | null } = $props();

  const intentConfig: Record<string, { label: string; style: string }> = {
    action_request: {
      label: 'Action',
      style: 'background: color-mix(in srgb, var(--iris-color-error) 12%, transparent); color: var(--iris-color-error);',
    },
    question: {
      label: 'Question',
      style: 'background: color-mix(in srgb, var(--iris-color-warning) 12%, transparent); color: var(--iris-color-warning);',
    },
    scheduling: {
      label: 'Scheduling',
      style: 'background: color-mix(in srgb, var(--iris-color-info) 12%, transparent); color: var(--iris-color-info);',
    },
    fyi: {
      label: 'FYI',
      style: 'background: color-mix(in srgb, var(--iris-color-text-faint) 12%, transparent); color: var(--iris-color-text-faint);',
    },
    sales: {
      label: 'Sales',
      style: 'background: color-mix(in srgb, var(--iris-color-text-faint) 8%, transparent); color: var(--iris-color-text-faint); font-size: 9px;',
    },
    newsletter: {
      label: 'Newsletter',
      style: 'background: color-mix(in srgb, var(--iris-color-text-faint) 8%, transparent); color: var(--iris-color-text-faint); font-size: 9px;',
    },
    social: {
      label: 'Social',
      style: 'background: color-mix(in srgb, var(--iris-color-success) 12%, transparent); color: var(--iris-color-success);',
    },
  };

  let config = $derived(intent ? intentConfig[intent] : null);
</script>

{#if config}
  <span
    class="flex-shrink-0 px-1.5 py-0.5 rounded text-[10px] font-medium leading-tight"
    style={config.style}
  >
    {config.label}
  </span>
{/if}
