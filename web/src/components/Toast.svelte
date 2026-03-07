<script lang="ts">
  let { message, visible, ondismiss }: { message: string; visible: boolean; ondismiss: () => void } = $props();

  $effect(() => {
    if (visible) {
      const timer = setTimeout(ondismiss, 4000);
      return () => clearTimeout(timer);
    }
  });
</script>

{#if visible}
  <div
    class="fixed bottom-4 right-4 z-50 px-4 py-3 rounded-lg shadow-lg text-sm max-w-sm animate-slide-in"
    style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border); color: var(--iris-color-text);"
  >
    {message}
  </div>
{/if}

<style>
  @keyframes slide-in {
    from { transform: translateY(20px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }
  .animate-slide-in {
    animation: slide-in var(--iris-transition-normal) ease-out;
  }
</style>
