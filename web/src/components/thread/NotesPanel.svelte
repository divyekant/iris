<script lang="ts">
  import { api, type ThreadNote } from '../../lib/api';
  import { StickyNote, Plus, Pencil, Trash2, Check, X } from 'lucide-svelte';

  let { threadId, alwaysOpen = false }: { threadId: string; alwaysOpen?: boolean } = $props();

  let notes = $state<ThreadNote[]>([]);
  let loading = $state(true);
  let open = $state(false);
  let newContent = $state('');
  let creating = $state(false);
  let editingId = $state<string | null>(null);
  let editContent = $state('');
  let deletingId = $state<string | null>(null);
  let error = $state('');

  async function loadNotes() {
    loading = true;
    error = '';
    try {
      const res = await api.threadNotes.list(threadId);
      notes = res.notes;
    } catch (e: any) {
      error = e.message || 'Failed to load notes';
    } finally {
      loading = false;
    }
  }

  async function handleCreate() {
    const content = newContent.trim();
    if (!content) return;
    creating = true;
    error = '';
    try {
      const note = await api.threadNotes.create(threadId, content);
      notes = [note, ...notes];
      newContent = '';
    } catch (e: any) {
      error = e.message || 'Failed to create note';
    } finally {
      creating = false;
    }
  }

  function startEdit(note: ThreadNote) {
    editingId = note.id;
    editContent = note.content;
  }

  function cancelEdit() {
    editingId = null;
    editContent = '';
  }

  async function handleUpdate(noteId: string) {
    const content = editContent.trim();
    if (!content) return;
    error = '';
    try {
      const updated = await api.threadNotes.update(threadId, noteId, content);
      notes = notes.map((n) => (n.id === noteId ? updated : n));
      editingId = null;
      editContent = '';
    } catch (e: any) {
      error = e.message || 'Failed to update note';
    }
  }

  function confirmDelete(noteId: string) {
    deletingId = noteId;
  }

  async function handleDelete(noteId: string) {
    error = '';
    try {
      await api.threadNotes.delete(threadId, noteId);
      notes = notes.filter((n) => n.id !== noteId);
      deletingId = null;
    } catch (e: any) {
      error = e.message || 'Failed to delete note';
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      handleCreate();
    }
  }

  function handleEditKeydown(e: KeyboardEvent, noteId: string) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      handleUpdate(noteId);
    }
    if (e.key === 'Escape') {
      cancelEdit();
    }
  }

  function formatTime(ts: number): string {
    const d = new Date(ts * 1000);
    const now = new Date();
    const diffMs = now.getTime() - d.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    if (diffMins < 1) return 'just now';
    if (diffMins < 60) return `${diffMins}m ago`;
    const diffHours = Math.floor(diffMins / 60);
    if (diffHours < 24) return `${diffHours}h ago`;
    const diffDays = Math.floor(diffHours / 24);
    if (diffDays < 7) return `${diffDays}d ago`;
    return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  }

  function toggle() {
    open = !open;
    if (open && notes.length === 0 && loading) {
      loadNotes();
    }
  }

  $effect(() => {
    if (threadId) {
      loadNotes();
    }
  });
</script>

<div class="notes-panel" style={alwaysOpen ? '' : 'border-bottom: 1px solid var(--iris-color-border);'}>
  {#if !alwaysOpen}
    <button
      class="notes-toggle px-4 py-2 w-full flex items-center gap-2 text-xs"
      onclick={toggle}
    >
      <span class="text-[10px]">{open ? '\u25BE' : '\u25B8'}</span>
      <StickyNote size={14} />
      <span>Notes</span>
      {#if notes.length > 0}
        <span class="notes-badge">{notes.length}</span>
      {/if}
    </button>
  {/if}

  {#if open || alwaysOpen}
    <div class="{alwaysOpen ? 'p-3' : 'px-4 pb-3'} space-y-2">
      {#if loading}
        <div class="flex items-center gap-2 py-2">
          <div class="w-3 h-3 rounded-full animate-spin" style="border: 2px solid var(--iris-color-border); border-top-color: var(--iris-color-primary);"></div>
          <span class="text-xs" style="color: var(--iris-color-text-faint);">Loading notes...</span>
        </div>
      {:else}
        {#if error}
          <p class="text-xs py-1" style="color: var(--iris-color-error);">{error}</p>
        {/if}

        <!-- Notes list -->
        {#each notes as note (note.id)}
          <div class="note-card rounded-lg px-3 py-2">
            {#if editingId === note.id}
              <!-- Edit mode -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <textarea
                class="w-full text-sm bg-transparent outline-none resize-y leading-relaxed rounded p-1"
                style="color: var(--iris-color-text); border: 1px solid var(--iris-color-primary);"
                bind:value={editContent}
                onkeydown={(e) => handleEditKeydown(e, note.id)}
                rows="3"
              ></textarea>
              <div class="flex items-center gap-1 mt-1">
                <button
                  class="p-1 rounded transition-colors"
                  style="color: var(--iris-color-success);"
                  onclick={() => handleUpdate(note.id)}
                  title="Save"
                >
                  <Check size={14} />
                </button>
                <button
                  class="p-1 rounded transition-colors"
                  style="color: var(--iris-color-text-faint);"
                  onclick={cancelEdit}
                  title="Cancel"
                >
                  <X size={14} />
                </button>
              </div>
            {:else if deletingId === note.id}
              <!-- Delete confirmation -->
              <div class="flex items-center gap-2">
                <span class="text-xs flex-1" style="color: var(--iris-color-text-muted);">Delete this note?</span>
                <button
                  class="text-xs px-2 py-0.5 rounded transition-colors"
                  style="background: var(--iris-color-error); color: var(--iris-color-bg);"
                  onclick={() => handleDelete(note.id)}
                >
                  Delete
                </button>
                <button
                  class="text-xs px-2 py-0.5 rounded transition-colors"
                  style="color: var(--iris-color-text-faint);"
                  onclick={() => (deletingId = null)}
                >
                  Cancel
                </button>
              </div>
            {:else}
              <!-- Display mode -->
              <div class="flex items-start gap-2">
                <p class="text-sm flex-1 whitespace-pre-wrap leading-relaxed" style="color: var(--iris-color-text);">{note.content}</p>
                <div class="flex items-center gap-0.5 shrink-0 note-actions">
                  <button
                    class="p-1 rounded transition-colors"
                    style="color: var(--iris-color-text-faint);"
                    onclick={() => startEdit(note)}
                    title="Edit"
                  >
                    <Pencil size={12} />
                  </button>
                  <button
                    class="p-1 rounded transition-colors note-delete-btn"
                    style="color: var(--iris-color-text-faint);"
                    onclick={() => confirmDelete(note.id)}
                    title="Delete"
                  >
                    <Trash2 size={12} />
                  </button>
                </div>
              </div>
              <span class="text-[10px] mt-1 block" style="color: var(--iris-color-text-faint);">
                {formatTime(note.created_at)}
                {#if note.updated_at > note.created_at}
                  (edited)
                {/if}
              </span>
            {/if}
          </div>
        {/each}

        {#if notes.length === 0}
          <p class="text-xs py-2" style="color: var(--iris-color-text-faint);">No notes yet. Add a private note below.</p>
        {/if}

        <!-- Add note input -->
        <div class="note-input-area rounded-lg p-2 mt-2">
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <textarea
            class="w-full text-sm bg-transparent outline-none resize-y leading-relaxed"
            style="color: var(--iris-color-text);"
            bind:value={newContent}
            onkeydown={handleKeydown}
            placeholder="Add a private note..."
            rows="2"
          ></textarea>
          <div class="flex items-center justify-end gap-2 mt-1">
            <span class="text-[10px]" style="color: var(--iris-color-text-faint);">
              {navigator.platform.includes('Mac') ? '\u2318' : 'Ctrl'}+Enter
            </span>
            <button
              class="add-note-btn px-3 py-1 text-xs rounded-md font-medium disabled:opacity-50 transition-colors flex items-center gap-1"
              onclick={handleCreate}
              disabled={creating || !newContent.trim()}
            >
              <Plus size={12} />
              {creating ? 'Adding...' : 'Add'}
            </button>
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .notes-toggle {
    color: var(--iris-color-primary);
    background: transparent;
    border: none;
    cursor: pointer;
    text-align: left;
  }
  .notes-toggle:hover {
    background: var(--iris-color-bg-surface);
  }
  .notes-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: var(--iris-radius-full);
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
    font-size: 10px;
    font-weight: 600;
  }
  .note-card {
    background: color-mix(in srgb, var(--iris-color-primary) 8%, var(--iris-color-bg));
    border: 1px solid color-mix(in srgb, var(--iris-color-primary) 20%, transparent);
  }
  .note-card:hover .note-actions {
    opacity: 1;
  }
  .note-actions {
    opacity: 0;
    transition: opacity var(--iris-transition-fast);
  }
  .note-delete-btn:hover {
    color: var(--iris-color-error) !important;
  }
  .note-input-area {
    background: var(--iris-color-bg-surface);
    border: 1px solid var(--iris-color-border);
  }
  .note-input-area:focus-within {
    border-color: var(--iris-color-primary);
  }
  .add-note-btn {
    background: var(--iris-color-primary);
    color: var(--iris-color-bg);
  }
  .add-note-btn:hover:not(:disabled) {
    background: var(--iris-color-primary-hover);
  }
</style>
