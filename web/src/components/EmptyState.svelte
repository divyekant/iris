<script lang="ts">
  interface Props {
    icon?: string;
    title: string;
    subtitle?: string;
  }
  let { icon = 'inbox', title, subtitle }: Props = $props();
</script>

<div class="flex flex-col items-center justify-center flex-1 gap-4 py-24">
  <div
    class="w-16 h-16 rounded-full flex items-center justify-center"
    style="background: var(--iris-color-bg-surface);"
  >
    <span class="text-2xl" style="color: var(--iris-color-text-faint);">
      {#if icon === 'inbox'}📥{:else if icon === 'search'}🔍{:else}📄{/if}
    </span>
  </div>
  <h2 class="text-xl font-semibold" style="color: var(--iris-color-text);">{title}</h2>
  {#if subtitle}
    <p class="text-sm text-center max-w-xs" style="color: var(--iris-color-text-muted);">{subtitle}</p>
  {/if}
</div>
