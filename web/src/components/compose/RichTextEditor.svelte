<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Editor } from '@tiptap/core';
  import StarterKit from '@tiptap/starter-kit';
  import Underline from '@tiptap/extension-underline';
  import { TextStyle, Color, FontFamily } from '@tiptap/extension-text-style';
  import Highlight from '@tiptap/extension-highlight';
  import Link from '@tiptap/extension-link';
  import Placeholder from '@tiptap/extension-placeholder';
  import {
    Bold, Italic, Underline as UnderlineIcon, Strikethrough,
    List, ListOrdered, Link as LinkIcon, RemoveFormatting,
    Palette, Highlighter, ChevronDown
  } from 'lucide-svelte';

  let {
    content = '',
    disabled = false,
    onchange,
  }: {
    content?: string;
    disabled?: boolean;
    onchange?: (html: string, text: string) => void;
  } = $props();

  let editorElement: HTMLDivElement | undefined = $state();
  let editor: Editor | undefined = $state();
  let showFontMenu = $state(false);
  let showSizeMenu = $state(false);
  let showColorPicker = $state(false);

  const fonts = [
    { label: 'Default', value: '' },
    { label: 'Inter', value: 'Inter' },
    { label: 'Arial', value: 'Arial' },
    { label: 'Georgia', value: 'Georgia' },
    { label: 'Courier New', value: 'Courier New' },
    { label: 'Times New Roman', value: 'Times New Roman' },
  ];

  const sizes = [
    { label: '12', value: '12px' },
    { label: '14', value: '14px' },
    { label: '16', value: '16px' },
    { label: '18', value: '18px' },
    { label: '20', value: '20px' },
    { label: '24', value: '24px' },
  ];

  const colors = [
    '#e0e0e0', '#DC2626', '#CA8A04', '#16A34A', '#2563EB', '#9333EA',
    '#888888', '#f87171', '#fbbf24', '#4ade80', '#60a5fa', '#c084fc',
  ];

  function currentFont(): string {
    if (!editor) return 'Default';
    const attrs = editor.getAttributes('textStyle');
    return attrs.fontFamily || 'Default';
  }

  function setFont(value: string) {
    if (!editor) return;
    if (value) {
      editor.chain().focus().setFontFamily(value).run();
    } else {
      editor.chain().focus().unsetFontFamily().run();
    }
    showFontMenu = false;
  }

  function setSize(value: string) {
    if (!editor) return;
    // TipTap doesn't have a built-in fontSize extension in starter-kit,
    // so we use CSS via textStyle mark
    editor.chain().focus().setMark('textStyle', { fontSize: value }).run();
    showSizeMenu = false;
  }

  function setColor(color: string) {
    if (!editor) return;
    editor.chain().focus().setColor(color).run();
    showColorPicker = false;
  }

  function toggleHighlight() {
    if (!editor) return;
    editor.chain().focus().toggleHighlight({ color: '#CA8A04' }).run();
  }

  function insertLink() {
    if (!editor) return;
    const url = prompt('Enter URL:');
    if (url) {
      editor.chain().focus().setLink({ href: url }).run();
    }
  }

  function clearFormatting() {
    if (!editor) return;
    editor.chain().focus().clearNodes().unsetAllMarks().run();
  }

  // Expose methods for parent to programmatically set content
  export function setContent(html: string) {
    if (editor && !editor.isDestroyed) {
      editor.commands.setContent(html);
    }
  }

  export function getHTML(): string {
    return editor?.getHTML() || '';
  }

  export function getText(): string {
    return editor?.getText() || '';
  }

  export function insertText(text: string) {
    if (editor && !editor.isDestroyed) {
      editor.commands.insertContent(text);
    }
  }

  onMount(() => {
    if (!editorElement) return;

    editor = new Editor({
      element: editorElement,
      extensions: [
        StarterKit.configure({
          heading: false,
          codeBlock: false,
          code: false,
          blockquote: { HTMLAttributes: { style: 'border-left: 3px solid #444; padding-left: 12px; color: #888;' } },
        }),
        Underline,
        TextStyle,
        Color,
        Highlight.configure({ multicolor: true }),
        FontFamily,
        Link.configure({
          openOnClick: false,
          HTMLAttributes: { style: 'color: var(--iris-color-primary); text-decoration: underline;' },
        }),
        Placeholder.configure({
          placeholder: 'Write your message...',
        }),
      ],
      content: content || '',
      editable: !disabled,
      onUpdate: ({ editor: e }) => {
        onchange?.(e.getHTML(), e.getText());
      },
      editorProps: {
        attributes: {
          class: 'rich-editor-content',
          style: 'outline: none; min-height: 200px; color: var(--iris-color-text); font-size: 14px; line-height: 1.6;',
        },
      },
    });
  });

  onDestroy(() => {
    editor?.destroy();
  });

  // React to disabled changes
  $effect(() => {
    if (editor && !editor.isDestroyed) {
      editor.setEditable(!disabled);
    }
  });
</script>

<!-- Toolbar -->
{#if editor}
<div class="flex items-center gap-0.5 px-2 py-1.5 border-b flex-wrap" style="border-color: var(--iris-color-border-subtle); background: var(--iris-color-bg-surface);">
  <!-- Font Family -->
  <div class="relative">
    <button
      class="flex items-center gap-1 px-2 py-1 rounded text-xs toolbar-btn"
      style="color: var(--iris-color-text-muted);"
      onclick={() => { showFontMenu = !showFontMenu; showSizeMenu = false; showColorPicker = false; }}
      disabled={disabled}
    >
      <span class="max-w-[60px] truncate">{currentFont()}</span>
      <ChevronDown size={10} />
    </button>
    {#if showFontMenu}
      <div class="absolute top-full left-0 mt-1 rounded-lg shadow-lg py-1 min-w-[140px] z-20" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);">
        {#each fonts as f}
          <button
            class="w-full text-left px-3 py-1.5 text-xs toolbar-dropdown-item"
            style="color: var(--iris-color-text-muted); font-family: {f.value || 'inherit'};"
            onclick={() => setFont(f.value)}
          >{f.label}</button>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Font Size -->
  <div class="relative">
    <button
      class="flex items-center gap-1 px-2 py-1 rounded text-xs toolbar-btn"
      style="color: var(--iris-color-text-muted);"
      onclick={() => { showSizeMenu = !showSizeMenu; showFontMenu = false; showColorPicker = false; }}
      disabled={disabled}
    >
      <span>14</span>
      <ChevronDown size={10} />
    </button>
    {#if showSizeMenu}
      <div class="absolute top-full left-0 mt-1 rounded-lg shadow-lg py-1 min-w-[60px] z-20" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);">
        {#each sizes as s}
          <button
            class="w-full text-left px-3 py-1.5 text-xs toolbar-dropdown-item"
            style="color: var(--iris-color-text-muted);"
            onclick={() => setSize(s.value)}
          >{s.label}</button>
        {/each}
      </div>
    {/if}
  </div>

  <div class="w-px h-4 mx-1" style="background: var(--iris-color-border-subtle);"></div>

  <!-- Bold -->
  <button
    class="p-1.5 rounded toolbar-btn"
    class:toolbar-active={editor.isActive('bold')}
    style="color: var(--iris-color-text-muted);"
    onclick={() => editor?.chain().focus().toggleBold().run()}
    disabled={disabled}
    title="Bold (Cmd+B)"
  ><Bold size={14} /></button>

  <!-- Italic -->
  <button
    class="p-1.5 rounded toolbar-btn"
    class:toolbar-active={editor.isActive('italic')}
    style="color: var(--iris-color-text-muted);"
    onclick={() => editor?.chain().focus().toggleItalic().run()}
    disabled={disabled}
    title="Italic (Cmd+I)"
  ><Italic size={14} /></button>

  <!-- Underline -->
  <button
    class="p-1.5 rounded toolbar-btn"
    class:toolbar-active={editor.isActive('underline')}
    style="color: var(--iris-color-text-muted);"
    onclick={() => editor?.chain().focus().toggleUnderline().run()}
    disabled={disabled}
    title="Underline (Cmd+U)"
  ><UnderlineIcon size={14} /></button>

  <!-- Strikethrough -->
  <button
    class="p-1.5 rounded toolbar-btn"
    class:toolbar-active={editor.isActive('strike')}
    style="color: var(--iris-color-text-muted);"
    onclick={() => editor?.chain().focus().toggleStrike().run()}
    disabled={disabled}
    title="Strikethrough"
  ><Strikethrough size={14} /></button>

  <div class="w-px h-4 mx-1" style="background: var(--iris-color-border-subtle);"></div>

  <!-- Text Color -->
  <div class="relative">
    <button
      class="p-1.5 rounded toolbar-btn"
      style="color: var(--iris-color-text-muted);"
      onclick={() => { showColorPicker = !showColorPicker; showFontMenu = false; showSizeMenu = false; }}
      disabled={disabled}
      title="Text Color"
    ><Palette size={14} /></button>
    {#if showColorPicker}
      <div class="absolute top-full left-0 mt-1 rounded-lg shadow-lg p-2 z-20 grid grid-cols-6 gap-1" style="background: var(--iris-color-bg-elevated); border: 1px solid var(--iris-color-border);">
        {#each colors as c}
          <button
            class="w-5 h-5 rounded-sm border"
            style="background: {c}; border-color: var(--iris-color-border-subtle);"
            onclick={() => setColor(c)}
          ></button>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Highlight -->
  <button
    class="p-1.5 rounded toolbar-btn"
    class:toolbar-active={editor.isActive('highlight')}
    style="color: var(--iris-color-text-muted);"
    onclick={toggleHighlight}
    disabled={disabled}
    title="Highlight"
  ><Highlighter size={14} /></button>

  <div class="w-px h-4 mx-1" style="background: var(--iris-color-border-subtle);"></div>

  <!-- Bullet List -->
  <button
    class="p-1.5 rounded toolbar-btn"
    class:toolbar-active={editor.isActive('bulletList')}
    style="color: var(--iris-color-text-muted);"
    onclick={() => editor?.chain().focus().toggleBulletList().run()}
    disabled={disabled}
    title="Bullet List"
  ><List size={14} /></button>

  <!-- Numbered List -->
  <button
    class="p-1.5 rounded toolbar-btn"
    class:toolbar-active={editor.isActive('orderedList')}
    style="color: var(--iris-color-text-muted);"
    onclick={() => editor?.chain().focus().toggleOrderedList().run()}
    disabled={disabled}
    title="Numbered List"
  ><ListOrdered size={14} /></button>

  <div class="w-px h-4 mx-1" style="background: var(--iris-color-border-subtle);"></div>

  <!-- Link -->
  <button
    class="p-1.5 rounded toolbar-btn"
    class:toolbar-active={editor.isActive('link')}
    style="color: var(--iris-color-text-muted);"
    onclick={insertLink}
    disabled={disabled}
    title="Insert Link"
  ><LinkIcon size={14} /></button>

  <!-- Clear Formatting -->
  <button
    class="p-1.5 rounded toolbar-btn"
    style="color: var(--iris-color-text-faint);"
    onclick={clearFormatting}
    disabled={disabled}
    title="Clear Formatting"
  ><RemoveFormatting size={14} /></button>
</div>
{/if}

<!-- Editor area -->
<div
  bind:this={editorElement}
  class="rich-editor-wrapper flex-1 overflow-y-auto px-1"
></div>

<style>
  .toolbar-btn {
    transition: all 120ms ease;
  }
  .toolbar-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent);
    color: var(--iris-color-text);
  }
  .toolbar-active {
    background: color-mix(in srgb, var(--iris-color-primary) 15%, transparent) !important;
    color: var(--iris-color-primary) !important;
  }
  .toolbar-dropdown-item:hover {
    background: color-mix(in srgb, var(--iris-color-primary) 10%, transparent);
  }

  /* TipTap editor styling */
  :global(.rich-editor-content) {
    padding: 8px 4px;
  }
  :global(.rich-editor-content p) {
    margin: 0 0 0.25em 0;
  }
  :global(.rich-editor-content ul),
  :global(.rich-editor-content ol) {
    padding-left: 1.5em;
    margin: 0.25em 0;
  }
  :global(.rich-editor-content li) {
    margin: 0.15em 0;
  }
  :global(.rich-editor-content blockquote) {
    margin: 0.5em 0;
  }
  :global(.rich-editor-content p.is-editor-empty:first-child::before) {
    content: attr(data-placeholder);
    float: left;
    color: var(--iris-color-text-faint);
    pointer-events: none;
    height: 0;
  }
  :global(.rich-editor-content mark) {
    border-radius: 2px;
    padding: 0 2px;
  }
</style>
