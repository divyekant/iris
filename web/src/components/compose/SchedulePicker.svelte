<script lang="ts">
  import { Sunrise, Sun, Calendar, CalendarPlus } from 'lucide-svelte';

  let {
    onpick,
    onclose,
  }: {
    onpick: (epochSeconds: number) => void;
    onclose: () => void;
  } = $props();

  let showCustom = $state(false);
  let customDatetime = $state('');

  function getTomorrowMorning(): number {
    const d = new Date();
    d.setDate(d.getDate() + 1);
    d.setHours(8, 0, 0, 0);
    return Math.floor(d.getTime() / 1000);
  }

  function getTomorrowAfternoon(): number {
    const d = new Date();
    d.setDate(d.getDate() + 1);
    d.setHours(13, 0, 0, 0);
    return Math.floor(d.getTime() / 1000);
  }

  function getNextMondayMorning(): number {
    const d = new Date();
    const dayOfWeek = d.getDay();
    // Days until next Monday: if Sunday (0) -> 1, Monday (1) -> 7, etc.
    const daysUntilMonday = dayOfWeek === 0 ? 1 : (8 - dayOfWeek);
    d.setDate(d.getDate() + daysUntilMonday);
    d.setHours(8, 0, 0, 0);
    return Math.floor(d.getTime() / 1000);
  }

  function formatPresetTime(epoch: number): string {
    const d = new Date(epoch * 1000);
    return d.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' }) +
      ' at ' +
      d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
  }

  function handleCustomSubmit() {
    if (!customDatetime) return;
    const d = new Date(customDatetime);
    const epoch = Math.floor(d.getTime() / 1000);
    if (epoch <= Math.floor(Date.now() / 1000)) return;
    onpick(epoch);
  }

  // Set min datetime to now (rounded to next minute)
  function getMinDatetime(): string {
    const d = new Date();
    d.setMinutes(d.getMinutes() + 1, 0, 0);
    return d.toISOString().slice(0, 16);
  }

  const presets = [
    {
      label: 'Tomorrow morning',
      sublabel: () => formatPresetTime(getTomorrowMorning()),
      icon: Sunrise,
      getTime: getTomorrowMorning,
    },
    {
      label: 'Tomorrow afternoon',
      sublabel: () => formatPresetTime(getTomorrowAfternoon()),
      icon: Sun,
      getTime: getTomorrowAfternoon,
    },
    {
      label: 'Monday morning',
      sublabel: () => formatPresetTime(getNextMondayMorning()),
      icon: Calendar,
      getTime: getNextMondayMorning,
    },
  ];

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onclose();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="schedule-backdrop" onclick={handleBackdropClick}>
  <div class="schedule-dropdown">
    <div class="schedule-header">
      <span class="schedule-title">Schedule Send</span>
    </div>

    {#if !showCustom}
      <div class="schedule-options">
        {#each presets as preset}
          <button
            class="schedule-option"
            onclick={() => onpick(preset.getTime())}
          >
            <span class="schedule-option-icon">
              <preset.icon size={16} />
            </span>
            <span class="schedule-option-content">
              <span class="schedule-option-label">{preset.label}</span>
              <span class="schedule-option-sublabel">{preset.sublabel()}</span>
            </span>
          </button>
        {/each}

        <button
          class="schedule-option"
          onclick={() => (showCustom = true)}
        >
          <span class="schedule-option-icon">
            <CalendarPlus size={16} />
          </span>
          <span class="schedule-option-content">
            <span class="schedule-option-label">Pick date & time...</span>
          </span>
        </button>
      </div>
    {:else}
      <div class="schedule-custom">
        <input
          type="datetime-local"
          bind:value={customDatetime}
          min={getMinDatetime()}
          class="schedule-datetime-input"
        />
        <div class="schedule-custom-actions">
          <button
            class="schedule-custom-btn schedule-custom-cancel"
            onclick={() => (showCustom = false)}
          >
            Back
          </button>
          <button
            class="schedule-custom-btn schedule-custom-confirm"
            onclick={handleCustomSubmit}
            disabled={!customDatetime}
          >
            Confirm
          </button>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .schedule-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
  }

  .schedule-dropdown {
    position: absolute;
    bottom: 100%;
    left: 0;
    margin-bottom: 4px;
    min-width: 260px;
    border-radius: var(--iris-radius-lg, 12px);
    background: var(--iris-color-bg-elevated);
    border: 1px solid var(--iris-color-border);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
    overflow: hidden;
  }

  .schedule-header {
    padding: 10px 14px 6px;
    border-bottom: 1px solid var(--iris-color-border);
  }

  .schedule-title {
    font-family: Inter, system-ui, sans-serif;
    font-size: 12px;
    font-weight: 600;
    color: var(--iris-color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .schedule-options {
    padding: 4px 0;
  }

  .schedule-option {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 14px;
    background: transparent;
    border: none;
    cursor: pointer;
    text-align: left;
    transition: background 120ms ease;
  }

  .schedule-option:hover {
    background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent);
  }

  .schedule-option-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--iris-radius-md, 8px);
    background: color-mix(in srgb, var(--iris-color-primary) 12%, transparent);
    color: var(--iris-color-primary);
    flex-shrink: 0;
  }

  .schedule-option-content {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .schedule-option-label {
    font-family: Inter, system-ui, sans-serif;
    font-size: 13px;
    font-weight: 500;
    color: var(--iris-color-text);
  }

  .schedule-option-sublabel {
    font-family: Inter, system-ui, sans-serif;
    font-size: 12px;
    color: var(--iris-color-text-faint);
  }

  .schedule-custom {
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .schedule-datetime-input {
    width: 100%;
    padding: 8px 10px;
    border-radius: var(--iris-radius-md, 8px);
    border: 1px solid var(--iris-color-border);
    background: var(--iris-color-bg-surface);
    color: var(--iris-color-text);
    font-family: Inter, system-ui, sans-serif;
    font-size: 13px;
    outline: none;
  }

  .schedule-datetime-input:focus {
    border-color: var(--iris-color-primary);
  }

  .schedule-custom-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  .schedule-custom-btn {
    padding: 6px 14px;
    border-radius: var(--iris-radius-md, 8px);
    font-family: Inter, system-ui, sans-serif;
    font-size: 13px;
    font-weight: 500;
    border: none;
    cursor: pointer;
    transition: all 120ms ease;
  }

  .schedule-custom-cancel {
    background: transparent;
    color: var(--iris-color-text-muted);
  }

  .schedule-custom-cancel:hover {
    background: var(--iris-color-bg-surface);
    color: var(--iris-color-text);
  }

  .schedule-custom-confirm {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }

  .schedule-custom-confirm:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .schedule-custom-confirm:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
