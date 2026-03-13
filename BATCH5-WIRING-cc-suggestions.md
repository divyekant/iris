# Wiring: Smart CC/BCC Suggestions (#54)

## 1. `src/api/mod.rs` — Register the module

Add after line 15 (`pub mod trust;`):

```rust
pub mod cc_suggestions;
```

## 2. `src/lib.rs` — Register the route

Add inside the `protected_api` Router chain (after the existing `/ai/chat/memory` route, around line 62):

```rust
.route("/ai/suggest-cc", post(api::cc_suggestions::suggest_cc))
```

## 3. `web/src/lib/api.ts` — Add API function

Add inside the `ai` namespace object (after the `reprocess` entry, around line 124):

```typescript
    suggestCc: (params: { thread_id?: string; to: string[]; cc: string[]; subject: string; body_preview: string }) =>
      request<{ suggestions: { email: string; name: string | null; reason: string; confidence: number; type: 'cc' | 'bcc' }[] }>('/api/ai/suggest-cc', {
        method: 'POST',
        body: JSON.stringify(params),
      }),
```

## 4. `web/src/components/compose/ComposeModal.svelte` — Integrate CcSuggestions

### Import (add after existing import):

```svelte
import CcSuggestions from './CcSuggestions.svelte';
```

### Add component after the Bcc input block (after the `{/if}` that closes `showCcBcc`, around line 295):

```svelte
      <CcSuggestions
        {to}
        {cc}
        {subject}
        bodyPreview={body}
        threadId={context.original?.message_id}
        onaddcc={(email) => {
          showCcBcc = true;
          cc = cc ? `${cc}, ${email}` : email;
        }}
        onaddbcc={(email) => {
          showCcBcc = true;
          bcc = bcc ? `${bcc}, ${email}` : email;
        }}
      />
```
