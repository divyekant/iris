<script lang="ts">
  let {
    action,
    messageCount,
    sampleSubjects,
    onconfirm,
    oncancel,
  }: {
    action: string;
    messageCount: number;
    sampleSubjects: string[];
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();

  const actionLabels: Record<string, string> = {
    archive: 'Archive',
    mark_read: 'Mark as Read',
    mark_unread: 'Mark as Unread',
    trash: 'Move to Trash',
    delete: 'Move to Trash',
    star: 'Star',
    unstar: 'Unstar',
    move_to_category: 'Move to Category',
  };

  const actionIcons: Record<string, string> = {
    archive: 'M20.25 7.5l-.625 10.632a2.25 2.25 0 0 1-2.247 2.118H6.622a2.25 2.25 0 0 1-2.247-2.118L3.75 7.5m8.25 3v6.75m0 0-3-3m3 3 3-3M3.375 7.5h17.25c.621 0 1.125-.504 1.125-1.125v-1.5c0-.621-.504-1.125-1.125-1.125H3.375c-.621 0-1.125.504-1.125 1.125v1.5c0 .621.504 1.125 1.125 1.125Z',
    mark_read: 'M21.75 9v.906a2.25 2.25 0 0 1-1.183 1.981l-6.478 3.488M2.25 9v.906a2.25 2.25 0 0 0 1.183 1.981l6.478 3.488m8.839 2.51-4.66-2.51m0 0-1.023-.55a2.25 2.25 0 0 0-2.134 0l-1.022.55m0 0-4.661 2.51m16.5 1.615a2.25 2.25 0 0 1-2.25 2.25h-15a2.25 2.25 0 0 1-2.25-2.25V8.844a2.25 2.25 0 0 1 1.183-1.981l7.5-4.04a2.25 2.25 0 0 1 2.134 0l7.5 4.04a2.25 2.25 0 0 1 1.183 1.98V19.5Z',
    mark_unread: 'M21.75 6.75v10.5a2.25 2.25 0 0 1-2.25 2.25h-15a2.25 2.25 0 0 1-2.25-2.25V6.75m19.5 0A2.25 2.25 0 0 0 19.5 4.5h-15a2.25 2.25 0 0 0-2.25 2.25m19.5 0v.243a2.25 2.25 0 0 1-1.07 1.916l-7.5 4.615a2.25 2.25 0 0 1-2.36 0L3.32 8.91a2.25 2.25 0 0 1-1.07-1.916V6.75',
    trash: 'M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0',
    delete: 'M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0',
    star: 'M11.48 3.499a.562.562 0 0 1 1.04 0l2.125 5.111a.563.563 0 0 0 .475.345l5.518.442c.499.04.701.663.321.988l-4.204 3.602a.563.563 0 0 0-.182.557l1.285 5.385a.562.562 0 0 1-.84.61l-4.725-2.885a.562.562 0 0 0-.586 0L6.982 20.54a.562.562 0 0 1-.84-.61l1.285-5.386a.562.562 0 0 0-.182-.557l-4.204-3.602a.562.562 0 0 1 .321-.988l5.518-.442a.563.563 0 0 0 .475-.345L11.48 3.5Z',
    unstar: 'M11.48 3.499a.562.562 0 0 1 1.04 0l2.125 5.111a.563.563 0 0 0 .475.345l5.518.442c.499.04.701.663.321.988l-4.204 3.602a.563.563 0 0 0-.182.557l1.285 5.385a.562.562 0 0 1-.84.61l-4.725-2.885a.562.562 0 0 0-.586 0L6.982 20.54a.562.562 0 0 1-.84-.61l1.285-5.386a.562.562 0 0 0-.182-.557l-4.204-3.602a.562.562 0 0 1 .321-.988l5.518-.442a.563.563 0 0 0 .475-.345L11.48 3.5Z',
    move_to_category: 'M9.568 3H5.25A2.25 2.25 0 0 0 3 5.25v4.318c0 .597.237 1.17.659 1.591l9.581 9.581c.699.699 1.78.872 2.607.33a18.095 18.095 0 0 0 5.223-5.223c.542-.827.369-1.908-.33-2.607L11.16 3.66A2.25 2.25 0 0 0 9.568 3Z',
  };

  let label = $derived(actionLabels[action] || action);
  let iconPath = $derived(actionIcons[action] || actionIcons['archive']);
</script>

<div
  class="rounded-lg p-3 mt-2"
  style="border: 1px solid var(--iris-color-border); background: var(--iris-color-bg-surface);"
>
  <!-- Header with icon and action label -->
  <div class="flex items-center gap-2 mb-2">
    <svg
      class="w-4 h-4 shrink-0"
      style="color: var(--iris-color-warning);"
      fill="none"
      viewBox="0 0 24 24"
      stroke-width="1.5"
      stroke="currentColor"
    >
      <path stroke-linecap="round" stroke-linejoin="round" d={iconPath} />
    </svg>
    <span class="text-xs font-semibold" style="color: var(--iris-color-text);">
      {label}
    </span>
    <span
      class="text-[10px] px-1.5 py-0.5 rounded-full font-medium"
      style="background: color-mix(in srgb, var(--iris-color-warning) 15%, transparent); color: var(--iris-color-warning);"
    >
      {messageCount} email{messageCount === 1 ? '' : 's'}
    </span>
  </div>

  <!-- Sample subjects -->
  {#if sampleSubjects.length > 0}
    <div class="mb-2 space-y-0.5">
      {#each sampleSubjects.slice(0, 5) as subject}
        <p
          class="text-[11px] truncate pl-6"
          style="color: var(--iris-color-text-muted);"
        >
          {subject}
        </p>
      {/each}
      {#if messageCount > 5}
        <p
          class="text-[11px] pl-6"
          style="color: var(--iris-color-text-faint);"
        >
          ...and {messageCount - 5} more
        </p>
      {/if}
    </div>
  {/if}

  <!-- Confirm / Cancel buttons -->
  <div class="flex gap-2 pl-6">
    <button
      class="px-3 py-1 text-xs font-medium rounded-md transition-opacity hover:opacity-90"
      style="background: var(--iris-color-primary); color: var(--iris-color-bg);"
      onclick={onconfirm}
    >
      Confirm
    </button>
    <button
      class="px-3 py-1 text-xs font-medium rounded-md transition-opacity hover:opacity-80"
      style="border: 1px solid var(--iris-color-border); color: var(--iris-color-text-muted); background: transparent;"
      onclick={oncancel}
    >
      Cancel
    </button>
  </div>
</div>
