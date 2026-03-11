<script lang="ts">
  import { Paperclip, Archive, Trash2, Star, Mail, MailOpen, Clock, ShieldAlert, Reply } from 'lucide-svelte';
  import SnoozePicker from '../SnoozePicker.svelte';

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
    labels?: string;
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

  const sentimentConfig: Record<string, { color: string; label: string }> = {
    positive: { color: 'var(--iris-color-success)', label: 'Positive' },
    negative: { color: 'var(--iris-color-error)', label: 'Negative' },
    neutral: { color: 'var(--iris-color-text-faint)', label: 'Neutral' },
    mixed: { color: 'var(--iris-color-warning)', label: 'Mixed' },
  };

  let parsedLabels: string[] = $derived.by(() => {
    if (!message.labels) return [];
    try { return JSON.parse(message.labels); } catch { return []; }
  });

  function handleCheckbox(e: Event) {
    e.stopPropagation();
    const checked = (e.target as HTMLInputElement).checked;
    onselect?.(message.id, checked);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
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
  <div class="flex-1 min-w-0">
    <div class="flex items-center gap-2">
      <span class="text-sm truncate {message.is_read ? '' : 'font-semibold'}" style="color: {message.is_read ? 'var(--iris-color-text-muted)' : 'var(--iris-color-text)'};">
        {senderDisplay}
      </span>
      {#if message.ai_category}
        <span class="flex-shrink-0 px-1.5 py-0.5 rounded text-[10px] font-medium" style="background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent); color: var(--iris-color-primary);">
          {message.ai_category}
        </span>
      {/if}
      {#if message.ai_sentiment && sentimentConfig[message.ai_sentiment]}
        <span
          class="flex-shrink-0 px-1.5 py-0.5 rounded text-[10px] font-medium"
          style="background: color-mix(in srgb, {sentimentConfig[message.ai_sentiment].color} 12%, transparent); color: {sentimentConfig[message.ai_sentiment].color};"
          title="Sentiment: {sentimentConfig[message.ai_sentiment].label}"
        >
          {sentimentConfig[message.ai_sentiment].label}
        </span>
      {/if}
      {#if message.ai_needs_reply}
        <span class="flex-shrink-0 inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded text-[10px] font-medium" style="background: color-mix(in srgb, var(--iris-color-warning) 12%, transparent); color: var(--iris-color-warning);" title="Needs reply">
          <Reply size={10} />Reply
        </span>
      {/if}
      {#each parsedLabels as label}
        <span class="flex-shrink-0 px-1.5 py-0.5 rounded text-[10px] font-medium"
          style="background: color-mix(in srgb, var(--iris-color-text-muted) 10%, transparent); color: var(--iris-color-text-muted);">
          {label}
        </span>
      {/each}
      <span class="flex-shrink-0 text-xs ml-auto flex items-center gap-1.5" style="color: var(--iris-color-text-faint);">
        {#if message.has_attachments}
          <span title="Has attachments"><Paperclip size={12} /></span>
        {/if}
        {formattedDate}
      </span>
    </div>
    <div class="text-sm truncate {message.is_read ? '' : 'font-medium'} flex items-center gap-1.5" style="color: {message.is_read ? 'var(--iris-color-text-muted)' : 'var(--iris-color-text)'};">
      <span class="truncate">{subjectDisplay}</span>
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

  <!-- Hover quick actions -->
  <div class="absolute right-2 top-1/2 -translate-y-1/2 hidden group-hover:flex items-center gap-1 px-2 py-1 rounded-lg"
       style="background: var(--iris-color-bg-elevated); box-shadow: 0 1px 3px rgba(0,0,0,.15);">
    <button
      class="p-1 rounded transition-colors quick-action"
      style="color: var(--iris-color-text-faint);"
      onclick={(e) => { e.stopPropagation(); onaction?.(message.id, 'archive'); }}
      title="Archive"
    ><Archive size={14} /></button>
    <button
      class="p-1 rounded transition-colors quick-action-delete"
      style="color: var(--iris-color-text-faint);"
      onclick={(e) => { e.stopPropagation(); onaction?.(message.id, 'delete'); }}
      title="Delete"
    ><Trash2 size={14} /></button>
    <button
      class="p-1 rounded transition-colors quick-action"
      style="color: var(--iris-color-text-faint);"
      onclick={(e) => { e.stopPropagation(); onaction?.(message.id, message.is_read ? 'mark_unread' : 'mark_read'); }}
      title={message.is_read ? 'Mark unread' : 'Mark read'}
    >{#if message.is_read}<Mail size={14} />{:else}<MailOpen size={14} />{/if}</button>
    <button
      class="p-1 rounded transition-colors quick-action"
      style="color: var(--iris-color-text-faint);"
      onclick={(e) => { e.stopPropagation(); onaction?.(message.id, 'star'); }}
      title="Star"
    ><Star size={14} /></button>
    <div class="relative">
      <button
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
    <button
      class="p-1 rounded transition-colors quick-action-spam"
      style="color: var(--iris-color-text-faint);"
      onclick={(e) => { e.stopPropagation(); onaction?.(message.id, 'report_spam'); }}
      title="Report Spam"
    ><ShieldAlert size={14} /></button>
  </div>
</div>

<style>
  .quick-action:hover { color: var(--iris-color-primary); }
  .quick-action-delete:hover { color: var(--iris-color-error); }
  .quick-action-snooze:hover { color: var(--iris-color-warning); }
  .quick-action-spam:hover { color: var(--iris-color-warning); }
</style>
