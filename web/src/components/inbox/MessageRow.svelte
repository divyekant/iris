<script lang="ts">
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
  }

  let { message, onclick, selected = false, onselect }: {
    message: Message;
    onclick: (id: string) => void;
    selected?: boolean;
    onselect?: (id: string, checked: boolean) => void;
  } = $props();

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

  function handleCheckbox(e: Event) {
    e.stopPropagation();
    const checked = (e.target as HTMLInputElement).checked;
    onselect?.(message.id, checked);
  }
</script>

<button
  class="w-full text-left px-4 py-3 border-b transition-colors flex items-start gap-3"
  style="border-color: var(--iris-color-border-subtle); {selected ? 'background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent);' : ''}"
  onclick={() => onclick(message.id)}
>
  <!-- Checkbox -->
  <div class="pt-1 flex-shrink-0">
    <input
      type="checkbox"
      checked={selected}
      onclick={handleCheckbox}
      class="w-4 h-4 rounded"
      style="accent-color: var(--iris-color-primary);"
    />
  </div>

  <!-- Unread indicator + priority badge -->
  <div class="pt-1.5 w-3 flex-shrink-0">
    {#if !message.is_read}
      <div class="w-2.5 h-2.5 rounded-full" style="background: var(--iris-color-unread);"></div>
    {:else if priorityStyle}
      <div class="w-2 h-2 rounded-full" style={priorityStyle} title={message.ai_priority_label}></div>
    {/if}
  </div>

  <!-- Content -->
  <div class="flex-1 min-w-0">
    <div class="flex items-baseline gap-2">
      <span class="text-sm truncate {message.is_read ? '' : 'font-semibold'}" style="color: {message.is_read ? 'var(--iris-color-text-muted)' : 'var(--iris-color-text)'};">
        {senderDisplay}
      </span>
      <span class="flex-shrink-0 text-xs ml-auto flex items-center gap-1.5" style="color: var(--iris-color-text-faint);">
        {#if message.ai_category}
          <span class="px-1.5 py-0.5 rounded text-[10px] font-medium" style="background: color-mix(in srgb, var(--iris-color-primary) 8%, transparent); color: var(--iris-color-primary);">
            {message.ai_category}
          </span>
        {/if}
        {#if message.has_attachments}
          <span title="Has attachments">&#128206;</span>
        {/if}
        {formattedDate}
      </span>
    </div>
    <div class="text-sm truncate {message.is_read ? '' : 'font-medium'}" style="color: {message.is_read ? 'var(--iris-color-text-muted)' : 'var(--iris-color-text)'};">
      {subjectDisplay}
    </div>
    {#if message.snippet}
      <div class="text-xs truncate mt-0.5" style="color: var(--iris-color-text-faint);">
        {message.snippet}
      </div>
    {/if}
  </div>
</button>
