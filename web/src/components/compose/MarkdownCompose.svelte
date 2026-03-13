<script lang="ts">
  import { api } from '../../lib/api';

  let {
    value = '',
    onchange,
  }: {
    value?: string;
    onchange?: (text: string, html: string | null) => void;
  } = $props();

  let markdownMode = $state(false);
  let previewHtml = $state('');
  let previewLoading = $state(false);
  let previewTab = $state<'edit' | 'preview'>('edit');
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let editorEl: HTMLTextAreaElement | undefined = $state();

  // Track narrow screen for tab layout vs side-by-side
  let narrow = $state(false);

  function checkWidth(el: HTMLDivElement) {
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        narrow = entry.contentRect.width < 560;
      }
    });
    ro.observe(el);
    return {
      destroy() {
        ro.disconnect();
      },
    };
  }

  async function fetchPreview(md: string) {
    if (!md.trim()) {
      previewHtml = '';
      return;
    }
    previewLoading = true;
    try {
      const res: { html: string } = await api.markdown.preview(md);
      previewHtml = res.html;
    } catch {
      previewHtml = '<p style="color: var(--iris-color-error);">Preview failed</p>';
    } finally {
      previewLoading = false;
    }
  }

  function handleInput(e: Event) {
    const target = e.target as HTMLTextAreaElement;
    value = target.value;
    onchange?.(value, null);

    if (markdownMode) {
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => fetchPreview(value), 300);
    }
  }

  function toggleMode() {
    markdownMode = !markdownMode;
    if (markdownMode && value.trim()) {
      fetchPreview(value);
    }
    if (!markdownMode) {
      previewHtml = '';
      previewTab = 'edit';
      // When switching back, notify parent with no HTML
      onchange?.(value, null);
    }
  }

  /** Convert markdown and pass HTML to parent (call before send). */
  export async function getHtml(): Promise<string | null> {
    if (!markdownMode || !value.trim()) return null;
    await fetchPreview(value);
    return previewHtml;
  }

  /** Whether markdown mode is currently active. */
  export function isMarkdownMode(): boolean {
    return markdownMode;
  }

  function handleKeydown(e: KeyboardEvent) {
    // Cmd+Shift+M to toggle markdown mode
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key === 'm') {
      e.preventDefault();
      toggleMode();
    }
  }

  $effect(() => {
    return () => {
      if (debounceTimer) clearTimeout(debounceTimer);
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="markdown-compose"
  use:checkWidth
  onkeydown={handleKeydown}
>
  <!-- Toolbar -->
  <div class="mc-toolbar">
    <button
      class="mc-toggle-btn"
      class:mc-active={markdownMode}
      onclick={toggleMode}
      title={`${navigator.platform.includes('Mac') ? '\u2318' : 'Ctrl'}+Shift+M`}
      type="button"
    >
      {markdownMode ? 'Rich Text' : 'Markdown'}
    </button>

    {#if markdownMode && narrow}
      <div class="mc-tabs">
        <button
          class="mc-tab"
          class:mc-tab-active={previewTab === 'edit'}
          onclick={() => (previewTab = 'edit')}
          type="button"
        >Edit</button>
        <button
          class="mc-tab"
          class:mc-tab-active={previewTab === 'preview'}
          onclick={() => {
            previewTab = 'preview';
            fetchPreview(value);
          }}
          type="button"
        >Preview</button>
      </div>
    {/if}
  </div>

  <!-- Editor area -->
  {#if markdownMode}
    <div class="mc-split" class:mc-narrow={narrow}>
      <!-- Editor pane -->
      {#if !narrow || previewTab === 'edit'}
        <div class="mc-pane mc-editor-pane">
          <textarea
            bind:this={editorEl}
            class="mc-editor"
            {value}
            oninput={handleInput}
            placeholder="Write markdown..."
            spellcheck="true"
          ></textarea>
        </div>
      {/if}

      <!-- Preview pane (side-by-side on wide, tab on narrow) -->
      {#if !narrow || previewTab === 'preview'}
        <div class="mc-pane mc-preview-pane">
          {#if previewLoading}
            <p class="mc-loading">Rendering...</p>
          {:else if previewHtml}
            <div class="mc-preview-content">
              {@html previewHtml}
            </div>
          {:else}
            <p class="mc-placeholder">Preview will appear here...</p>
          {/if}
        </div>
      {/if}
    </div>
  {:else}
    <!-- Plain textarea mode (same as original ComposeModal) -->
    <textarea
      class="mc-plain-editor"
      {value}
      oninput={handleInput}
      placeholder="Write your message..."
    ></textarea>
  {/if}
</div>

<style>
  .markdown-compose {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }

  /* Toolbar */
  .mc-toolbar {
    display: flex;
    align-items: center;
    gap: calc(var(--iris-spacing-base) * 2);
    padding: calc(var(--iris-spacing-base) * 1) 0;
  }

  .mc-toggle-btn {
    font-size: 0.75rem;
    padding: calc(var(--iris-spacing-base) * 1) calc(var(--iris-spacing-base) * 2);
    border-radius: var(--iris-radius-sm);
    border: 1px solid var(--iris-color-border);
    background: transparent;
    color: var(--iris-color-text-muted);
    cursor: pointer;
    transition: all var(--iris-transition-fast);
    font-family: var(--iris-font-family);
  }

  .mc-toggle-btn:hover {
    background: var(--iris-color-bg-surface);
    color: var(--iris-color-text);
  }

  .mc-toggle-btn.mc-active {
    background: color-mix(in srgb, var(--iris-color-primary) 15%, transparent);
    border-color: var(--iris-color-primary);
    color: var(--iris-color-primary);
  }

  /* Tabs for narrow screens */
  .mc-tabs {
    display: flex;
    gap: calc(var(--iris-spacing-base) * 1);
  }

  .mc-tab {
    font-size: 0.75rem;
    padding: calc(var(--iris-spacing-base) * 1) calc(var(--iris-spacing-base) * 2);
    border: none;
    background: transparent;
    color: var(--iris-color-text-faint);
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition: all var(--iris-transition-fast);
    font-family: var(--iris-font-family);
  }

  .mc-tab:hover {
    color: var(--iris-color-text-muted);
  }

  .mc-tab-active {
    color: var(--iris-color-primary);
    border-bottom-color: var(--iris-color-primary);
  }

  /* Split layout */
  .mc-split {
    display: flex;
    flex: 1;
    min-height: 0;
    gap: calc(var(--iris-spacing-base) * 2);
  }

  .mc-split.mc-narrow {
    flex-direction: column;
  }

  .mc-pane {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .mc-editor-pane {
    border: 1px solid var(--iris-color-border);
    border-radius: var(--iris-radius-md);
    overflow: hidden;
  }

  .mc-preview-pane {
    border: 1px solid var(--iris-color-border-subtle);
    border-radius: var(--iris-radius-md);
    overflow-y: auto;
    padding: calc(var(--iris-spacing-base) * 3);
    background: var(--iris-color-bg-surface);
  }

  /* Editor textarea (markdown mode) */
  .mc-editor {
    width: 100%;
    height: 100%;
    min-height: 200px;
    resize: none;
    border: none;
    outline: none;
    padding: calc(var(--iris-spacing-base) * 3);
    font-family: var(--iris-font-mono);
    font-size: 0.875rem;
    line-height: 1.6;
    color: var(--iris-color-text);
    background: transparent;
    tab-size: 2;
  }

  .mc-editor::placeholder {
    color: var(--iris-color-text-faint);
  }

  /* Plain textarea (rich text mode — same as original) */
  .mc-plain-editor {
    width: 100%;
    min-height: 200px;
    resize: vertical;
    border: none;
    outline: none;
    font-size: 0.875rem;
    line-height: 1.6;
    color: var(--iris-color-text);
    background: transparent;
    font-family: var(--iris-font-family);
  }

  .mc-plain-editor::placeholder {
    color: var(--iris-color-text-faint);
  }

  /* Preview content */
  .mc-preview-content {
    font-size: 0.875rem;
    line-height: 1.6;
    color: var(--iris-color-text);
    word-wrap: break-word;
  }

  .mc-preview-content :global(h1),
  .mc-preview-content :global(h2),
  .mc-preview-content :global(h3),
  .mc-preview-content :global(h4),
  .mc-preview-content :global(h5),
  .mc-preview-content :global(h6) {
    color: var(--iris-color-text);
    margin: calc(var(--iris-spacing-base) * 4) 0 calc(var(--iris-spacing-base) * 2);
    line-height: 1.3;
  }

  .mc-preview-content :global(h1) { font-size: 1.5rem; }
  .mc-preview-content :global(h2) { font-size: 1.25rem; }
  .mc-preview-content :global(h3) { font-size: 1.1rem; }

  .mc-preview-content :global(p) {
    margin: calc(var(--iris-spacing-base) * 2) 0;
  }

  .mc-preview-content :global(ul),
  .mc-preview-content :global(ol) {
    margin: calc(var(--iris-spacing-base) * 2) 0;
    padding-left: calc(var(--iris-spacing-base) * 6);
  }

  .mc-preview-content :global(li) {
    margin: calc(var(--iris-spacing-base) * 1) 0;
  }

  .mc-preview-content :global(blockquote) {
    margin: calc(var(--iris-spacing-base) * 2) 0;
    padding: calc(var(--iris-spacing-base) * 2) calc(var(--iris-spacing-base) * 4);
    border-left: 3px solid var(--iris-color-primary);
    background: color-mix(in srgb, var(--iris-color-bg-surface) 50%, transparent);
    color: var(--iris-color-text-muted);
  }

  .mc-preview-content :global(code) {
    font-family: var(--iris-font-mono);
    font-size: 0.8125rem;
    background: var(--iris-color-bg-elevated);
    padding: 1px calc(var(--iris-spacing-base) * 1);
    border-radius: var(--iris-radius-sm);
    color: var(--iris-color-primary);
  }

  .mc-preview-content :global(pre) {
    margin: calc(var(--iris-spacing-base) * 2) 0;
    padding: calc(var(--iris-spacing-base) * 3);
    background: var(--iris-color-bg-elevated);
    border-radius: var(--iris-radius-md);
    overflow-x: auto;
  }

  .mc-preview-content :global(pre code) {
    background: transparent;
    padding: 0;
    color: var(--iris-color-text);
  }

  .mc-preview-content :global(a) {
    color: var(--iris-color-primary);
    text-decoration: underline;
  }

  .mc-preview-content :global(a:hover) {
    color: var(--iris-color-secondary);
  }

  .mc-preview-content :global(table) {
    width: 100%;
    border-collapse: collapse;
    margin: calc(var(--iris-spacing-base) * 2) 0;
  }

  .mc-preview-content :global(th),
  .mc-preview-content :global(td) {
    padding: calc(var(--iris-spacing-base) * 2);
    border: 1px solid var(--iris-color-border);
    text-align: left;
  }

  .mc-preview-content :global(th) {
    background: var(--iris-color-bg-elevated);
    font-weight: 600;
  }

  .mc-preview-content :global(hr) {
    border: none;
    border-top: 1px solid var(--iris-color-border);
    margin: calc(var(--iris-spacing-base) * 4) 0;
  }

  .mc-preview-content :global(img) {
    max-width: 100%;
    border-radius: var(--iris-radius-md);
  }

  /* States */
  .mc-loading {
    font-size: 0.75rem;
    color: var(--iris-color-text-faint);
    font-style: italic;
  }

  .mc-placeholder {
    font-size: 0.75rem;
    color: var(--iris-color-text-faint);
  }
</style>
