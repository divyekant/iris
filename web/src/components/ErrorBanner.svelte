<script lang="ts">
  interface Props {
    message: string;
    actionLabel?: string;
    onaction?: () => void;
    ondismiss?: () => void;
  }
  let { message, actionLabel = 'Reconnect', onaction, ondismiss }: Props = $props();
</script>

<div
  class="flex items-center gap-3 px-6 h-12"
  style="background: color-mix(in srgb, var(--iris-color-error) 8%, transparent);"
>
  <span style="color: var(--iris-color-error);">⚠</span>
  <p class="text-[13px] flex-1" style="color: var(--iris-color-text);">{message}</p>
  {#if onaction}
    <button
      class="px-3.5 py-1.5 rounded-lg text-xs font-semibold"
      style="background: var(--iris-color-error); color: white;"
      onclick={onaction}
    >{actionLabel}</button>
  {/if}
  {#if ondismiss}
    <button
      class="text-sm"
      style="color: var(--iris-color-text-faint);"
      onclick={ondismiss}
    >✕</button>
  {/if}
</div>
