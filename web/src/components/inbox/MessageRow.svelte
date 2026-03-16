<script lang="ts">
  import { Paperclip, Archive, Trash2, Star, Clock } from 'lucide-svelte';
  import SnoozePicker from '../SnoozePicker.svelte';
  import Badge from '$components/shared/Badge.svelte';
  import { getPrimaryBadges } from '$lib/badge-priority';
  import { irisCollapse, staggeredFade } from '$lib/transitions';

  interface Message {
    id: string;
    from_name?: string;
    from_address: string;
    subject?: string;
    snippet?: string;
    date: number | string;
    is_read: boolean;
    has_attachments?: boolean;
    ai_priority_label?: string;
    ai_category?: string;
    ai_sentiment?: string;
    ai_needs_reply?: boolean;
    intent?: string;
    labels?: string;
    has_auto_draft?: boolean;
  }

  let { message, onclick, selected = false, focused = false, onselect, onaction, onsnooze, muted = false }: {
    message: Message;
    onclick: (id: string) => void;
    selected?: boolean;
    focused?: boolean;
    onselect?: (id: string, checked: boolean) => void;
    onaction?: (id: string, action: string) => void;
    onsnooze?: (id: string, snoozeUntil: number) => void;
    muted?: boolean;
  } = $props();

  let snoozePickerOpen = $state(false);

  let senderDisplay = $derived(message.from_name || message.from_address);
  let subjectDisplay = $derived(message.subject || '(no subject)');

  let formattedDate = $derived.by(() => {
    const raw = message.date;
    const msgDate = new Date(typeof raw === 'number' && raw < 1e12 ? raw * 1000 : raw);
    const now = new Date();
    const isToday =
      msgDate.getFullYear() === now.getFullYear() &&
      msgDate.getMonth() === now.getMonth() &&
      msgDate.getDate() === now.getDate();

    if (isToday) {
      return msgDate.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    }
    return msgDate.toLocaleDateString([], { month: 'short', day: 'numeric' });
  });

  const priorityStyles: Record<string, string> = {
    urgent: 'background: var(--iris-color-error);',
    high: 'background: var(--iris-color-warning);',
    normal: 'background: var(--iris-color-success);',
    low: 'background: var(--iris-color-text-faint);',
  };

  let priorityStyle = $derived(
    message.ai_priority_label ? priorityStyles[message.ai_priority_label] || '' : ''
  );

  // Map AI fields to badge-priority interface (ai_* prefix → clean names)
  let badgeResult = $derived(getPrimaryBadges({
    needs_reply: message.ai_needs_reply,
    intent: message.intent,
    sentiment: message.ai_sentiment,
    category: message.ai_category,
    labels: message.labels,
  }));

  function handleCheckbox(e: Event) {
    e.stopPropagation();
    const checked = (e.target as HTMLInputElement).checked;
    onselect?.(message.id, checked);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  out:irisCollapse
  class="group relative w-full text-left px-4 py-3 border-b transition-colors flex items-center gap-2 cursor-pointer"
  style="border-color: var(--iris-color-border-subtle); {selected ? 'background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent);' : focused ? 'background: color-mix(in srgb, var(--iris-color-primary) 6%, transparent);' : ''}"
  role="button"
  tabindex="0"
  onclick={() => onclick(message.id)}
  onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onclick(message.id); } }}
>
  <!-- Checkbox -->
  <div class="flex-shrink-0">
    <input
      type="checkbox"
      checked={selected}
      onclick={handleCheckbox}
      class="w-4 h-4 rounded"
      style="accent-color: var(--iris-color-primary);"
    />
  </div>

  <!-- Unread indicator + priority badge -->
  <div class="w-2.5 flex-shrink-0 flex items-center justify-center">
    {#if !message.is_read}
      <div class="w-2 h-2 rounded-full" style="background: var(--iris-color-unread);"></div>
    {:else if priorityStyle}
      <div class="w-2 h-2 rounded-full" style={priorityStyle} title={message.ai_priority_label}></div>
    {/if}
  </div>

  <!-- Content -->
  <div class="flex-1 min-w-0 overflow-hidden">
    <div class="flex items-center gap-2">
      <span class="text-sm truncate flex-shrink {message.is_read ? '' : 'font-semibold'}" style="color: {message.is_read ? 'var(--iris-color-text-muted)' : 'var(--iris-color-text)'}; min-width: 60px;">
        {senderDisplay}
      </span>
      <!-- Smart Badge Priority: show 1 primary badge + overflow count -->
      <div class="flex items-center gap-1 flex-shrink-0">
        {#if badgeResult.primary}
          <Badge
            variant={badgeResult.primary.type}
            text={badgeResult.primary.label}
            size="sm"
          />
        {/if}
        {#if badgeResult.overflow > 0}
          <span
            class="flex-shrink-0 px-1.5 py-0.5 rounded text-[10px] font-medium"
            style="background: color-mix(in srgb, var(--iris-color-text-faint) 10%, transparent); color: var(--iris-color-text-faint);"
            title="{badgeResult.overflow} more badge{badgeResult.overflow > 1 ? 's' : ''}"
          >+{badgeResult.overflow}</span>
        {/if}
      </div>
      <span class="flex-shrink-0 text-xs ml-auto flex items-center gap-1.5" style="color: var(--iris-color-text-faint);">
        {#if message.has_attachments}
          <span title="Has attachments"><Paperclip size={12} /></span>
        {/if}
        {formattedDate}
      </span>
    </div>
    <div class="text-sm truncate {message.is_read ? '' : 'font-medium'} flex items-center gap-1.5" style="color: {message.is_read ? 'var(--iris-color-text-muted)' : 'var(--iris-color-text)'};">
      <span class="truncate">{subjectDisplay}</span>
      {#if message.has_auto_draft}
        <button
          class="flex-shrink-0 px-1.5 py-0.5 rounded text-[9px] font-medium uppercase cursor-pointer draft-ready-chip border-0"
          style="background: color-mix(in srgb, var(--iris-color-info) 15%, transparent); color: var(--iris-color-info);"
          title="Auto-draft reply available"
          onclick={(e) => { e.stopPropagation(); }}
        >Draft ready</button>
      {/if}
      {#if muted}
        <span class="flex-shrink-0 px-1.5 py-0.5 rounded text-[9px] font-medium uppercase" style="background: color-mix(in srgb, var(--iris-color-text-faint) 15%, transparent); color: var(--iris-color-text-faint);">Muted</span>
      {/if}
    </div>
    {#if message.snippet}
      <div class="text-xs truncate mt-0.5" style="color: var(--iris-color-text-faint);">
        {message.snippet}
      </div>
    {/if}
  </div>

  <!-- Hover quick actions (streamlined: Archive, Delete, Star, Snooze) -->
  <div class="absolute right-2 top-1/2 -translate-y-1/2 hidden group-hover:flex items-center gap-1 px-2 py-1 rounded-lg"
       style="background: var(--iris-color-bg-elevated); box-shadow: 0 1px 3px rgba(0,0,0,.15);">
    <button
      transition:staggeredFade={{ index: 0 }}
      class="p-1 rounded transition-colors quick-action"
      style="color: var(--iris-color-text-faint);"
      onclick={(e) => { e.stopPropagation(); onaction?.(message.id, 'archive'); }}
      title="Archive"
    ><Archive size={14} /></button>
    <button
      transition:staggeredFade={{ index: 1 }}
      class="p-1 rounded transition-colors quick-action-delete"
      style="color: var(--iris-color-text-faint);"
      onclick={(e) => { e.stopPropagation(); onaction?.(message.id, 'delete'); }}
      title="Delete"
    ><Trash2 size={14} /></button>
    <button
      transition:staggeredFade={{ index: 2 }}
      class="p-1 rounded transition-colors quick-action"
      style="color: var(--iris-color-text-faint);"
      onclick={(e) => { e.stopPropagation(); onaction?.(message.id, 'star'); }}
      title="Star"
    ><Star size={14} /></button>
    <div class="relative">
      <button
        transition:staggeredFade={{ index: 3 }}
        class="p-1 rounded transition-colors quick-action-snooze"
        style="color: var(--iris-color-text-faint);"
        onclick={(e) => { e.stopPropagation(); snoozePickerOpen = !snoozePickerOpen; }}
        title="Snooze"
      ><Clock size={14} /></button>
      {#if snoozePickerOpen}
        <div class="absolute right-0 top-full mt-1">
          <SnoozePicker
            onpick={(epoch) => { snoozePickerOpen = false; onsnooze?.(message.id, epoch); }}
            onclose={() => { snoozePickerOpen = false; }}
          />
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .quick-action:hover { color: var(--iris-color-primary); }
  .quick-action-delete:hover { color: var(--iris-color-error); }
  .quick-action-snooze:hover { color: var(--iris-color-warning); }
  .draft-ready-chip:hover { filter: brightness(1.2); }
</style>
