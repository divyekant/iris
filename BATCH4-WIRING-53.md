# Feature #53 — Context-Aware Autocomplete — Wiring

## Route Registration

Add to `src/lib.rs` in the `protected_api` router:

```rust
.route("/ai/autocomplete", post(api::autocomplete::autocomplete))
```

## Module Registration

Already added to `src/api/mod.rs`:

```rust
pub mod autocomplete;
```

## Frontend Integration

The `AutocompleteTextarea` component is a drop-in replacement for a `<textarea>` in `ComposeModal.svelte`. To integrate:

```svelte
<!-- In ComposeModal.svelte, replace the body textarea with: -->
<AutocompleteTextarea
  bind:value={body}
  threadId={context.original?.message_id ? resolveThreadId(context) : null}
  composeMode={context.mode}
  placeholder="Write your message..."
  rows={8}
  oninput={scheduleAutoSave}
/>
```

## No Migration Required

This feature is stateless — all completions are generated in real-time via the AI provider pool. No database tables or migrations needed.

## Files Created

- `src/api/autocomplete.rs` — backend handler + unit tests
- `web/src/components/compose/AutocompleteDropdown.svelte` — floating suggestion dropdown
- `web/src/components/compose/AutocompleteTextarea.svelte` — textarea wrapper with debounced autocomplete
- `web/src/lib/api.ts` — added `AutocompleteSuggestion` type and `api.ai.autocomplete()` method
