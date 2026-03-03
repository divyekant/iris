<script lang="ts">
  interface Message {
    id: string;
    from_name?: string;
    from_address: string;
    subject?: string;
    snippet?: string;
    date: string;
    is_read: boolean;
    has_attachments?: boolean;
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
    const msgDate = new Date(message.date);
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

  function handleCheckbox(e: Event) {
    e.stopPropagation();
    const checked = (e.target as HTMLInputElement).checked;
    onselect?.(message.id, checked);
  }
</script>

<button
  class="w-full text-left px-4 py-3 border-b border-gray-100 dark:border-gray-800 hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors flex items-start gap-3
         {selected ? 'bg-blue-50 dark:bg-blue-900/20' : ''}"
  onclick={() => onclick(message.id)}
>
  <!-- Checkbox -->
  <div class="pt-1 flex-shrink-0">
    <input
      type="checkbox"
      checked={selected}
      onclick={handleCheckbox}
      class="w-4 h-4 rounded border-gray-300 dark:border-gray-600 text-blue-600 focus:ring-blue-500"
    />
  </div>

  <!-- Unread indicator -->
  <div class="pt-1.5 w-3 flex-shrink-0">
    {#if !message.is_read}
      <div class="w-2.5 h-2.5 rounded-full bg-blue-500"></div>
    {/if}
  </div>

  <!-- Content -->
  <div class="flex-1 min-w-0">
    <div class="flex items-baseline gap-2">
      <span class="text-sm truncate {message.is_read ? 'text-gray-700 dark:text-gray-300' : 'font-semibold text-gray-900 dark:text-gray-100'}">
        {senderDisplay}
      </span>
      <span class="flex-shrink-0 text-xs text-gray-400 dark:text-gray-500 ml-auto">
        {#if message.has_attachments}
          <span class="mr-1.5" title="Has attachments">{'\u{1F4CE}'}</span>
        {/if}
        {formattedDate}
      </span>
    </div>
    <div class="text-sm truncate {message.is_read ? 'text-gray-600 dark:text-gray-400' : 'font-medium text-gray-800 dark:text-gray-200'}">
      {subjectDisplay}
    </div>
    {#if message.snippet}
      <div class="text-xs text-gray-400 dark:text-gray-500 truncate mt-0.5">
        {message.snippet}
      </div>
    {/if}
  </div>
</button>
