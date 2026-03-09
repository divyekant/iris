<script lang="ts">
  import { Clock } from 'lucide-svelte';

  let { onpick, onclose }: {
    onpick: (epochSeconds: number) => void;
    onclose: () => void;
  } = $props();

  let customDate = $state('');
  let customTime = $state('09:00');

  function laterToday(): number {
    const d = new Date();
    d.setHours(d.getHours() + 3);
    return Math.floor(d.getTime() / 1000);
  }

  function tomorrowMorning(): number {
    const d = new Date();
    d.setDate(d.getDate() + 1);
    d.setHours(9, 0, 0, 0);
    return Math.floor(d.getTime() / 1000);
  }

  function nextWeek(): number {
    const d = new Date();
    // Next Monday
    const day = d.getDay();
    const diff = day === 0 ? 1 : 8 - day;
    d.setDate(d.getDate() + diff);
    d.setHours(9, 0, 0, 0);
    return Math.floor(d.getTime() / 1000);
  }

  function handleCustomPick() {
    if (!customDate) return;
    const [year, month, day] = customDate.split('-').map(Number);
    const [hours, minutes] = customTime.split(':').map(Number);
    const d = new Date(year, month - 1, day, hours, minutes, 0, 0);
    const epoch = Math.floor(d.getTime() / 1000);
    if (epoch > Math.floor(Date.now() / 1000)) {
      onpick(epoch);
    }
  }

  function formatPreview(epoch: number): string {
    const d = new Date(epoch * 1000);
    return d.toLocaleString([], {
      weekday: 'short',
      month: 'short',
      day: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
    });
  }

  const presets = [
    { label: 'Later today', sublabel: () => formatPreview(laterToday()), action: () => onpick(laterToday()) },
    { label: 'Tomorrow morning', sublabel: () => formatPreview(tomorrowMorning()), action: () => onpick(tomorrowMorning()) },
    { label: 'Next week', sublabel: () => formatPreview(nextWeek()), action: () => onpick(nextWeek()) },
  ];
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="w-64 rounded-xl p-2 z-50"
  style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border); box-shadow: 0 4px 12px rgba(0,0,0,.2);"
  onclick={(e) => e.stopPropagation()}
>
  <div class="flex items-center gap-2 px-2 py-1.5 mb-1">
    <Clock size={14} style="color: var(--iris-color-warning);" />
    <span class="text-xs font-semibold" style="color: var(--iris-color-text);">Snooze until</span>
    <span class="flex-1"></span>
    <button
      class="text-xs px-1.5 py-0.5 rounded"
      style="color: var(--iris-color-text-faint);"
      onclick={onclose}
    >&times;</button>
  </div>

  {#each presets as preset}
    <button
      class="flex items-center justify-between w-full px-2.5 py-2 rounded-lg text-left snooze-option"
      onclick={preset.action}
    >
      <span class="text-[13px]" style="color: var(--iris-color-text);">{preset.label}</span>
      <span class="text-[11px]" style="color: var(--iris-color-text-faint);">{preset.sublabel()}</span>
    </button>
  {/each}

  <div class="my-1.5" style="border-top: 1px solid var(--iris-color-border);"></div>

  <div class="px-2.5 py-1.5 space-y-2">
    <span class="text-[11px] font-medium" style="color: var(--iris-color-text-faint);">Pick date & time</span>
    <div class="flex items-center gap-2">
      <input
        type="date"
        bind:value={customDate}
        class="flex-1 text-xs rounded-md px-2 py-1.5 bg-transparent outline-none"
        style="color: var(--iris-color-text); border: 1px solid var(--iris-color-border);"
      />
      <input
        type="time"
        bind:value={customTime}
        class="w-20 text-xs rounded-md px-2 py-1.5 bg-transparent outline-none"
        style="color: var(--iris-color-text); border: 1px solid var(--iris-color-border);"
      />
    </div>
    <button
      class="w-full py-1.5 text-xs font-medium rounded-lg transition-colors disabled:opacity-40 snooze-confirm-btn"
      disabled={!customDate}
      onclick={handleCustomPick}
    >Snooze</button>
  </div>
</div>

<style>
  .snooze-option:hover {
    background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent);
  }
  .snooze-confirm-btn {
    background: var(--iris-color-warning);
    color: var(--iris-color-bg);
  }
  .snooze-confirm-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }
</style>
