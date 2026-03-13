# Batch 5 Wiring — Markdown Compose (#58)

## 1. `src/api/mod.rs` — add module declaration

Add this line after the existing modules (alphabetical placement near `messages`):

```rust
pub mod markdown;
```

## 2. `src/lib.rs` — add route to protected API

Add this route inside the `protected_api` Router (suggested placement: after the `/drafts/{id}` route):

```rust
.route("/compose/markdown-preview", post(api::markdown::markdown_preview))
```

## 3. `web/src/lib/api.ts` — add markdown API namespace

Add this inside the `api` object (suggested placement: after the `drafts` namespace):

```typescript
markdown: {
  preview: (markdown: string) =>
    request<{ html: string }>('/api/compose/markdown-preview', {
      method: 'POST',
      body: JSON.stringify({ markdown }),
    }),
},
```

## 4. `web/src/components/compose/ComposeModal.svelte` — integrate MarkdownCompose

### Import (add at top of `<script>` block):

```typescript
import MarkdownCompose from './MarkdownCompose.svelte';
```

### Add ref (add with other state declarations):

```typescript
let markdownCompose: MarkdownCompose | undefined = $state();
```

### Replace the body `<textarea>` in the form section with:

```svelte
<MarkdownCompose
  bind:this={markdownCompose}
  value={body}
  onchange={(text, html) => {
    body = text;
    scheduleAutoSave();
  }}
/>
```

### Update `handleSend` — before the `api.send(req)` call, add markdown HTML conversion:

```typescript
// Convert markdown to HTML if in markdown mode
if (markdownCompose?.isMarkdownMode()) {
  const mdHtml = await markdownCompose.getHtml();
  if (mdHtml) {
    req.body_html = mdHtml;
  }
}
```

### Update `handleSaveDraft` — add markdown HTML to draft saves:

```typescript
// In the api.drafts.save call, add body_html if in markdown mode
body_html: markdownCompose?.isMarkdownMode() ? undefined : undefined,
// (Optional: could also save markdown preview HTML to draft)
```
