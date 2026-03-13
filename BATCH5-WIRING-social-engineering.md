# Wiring Instructions for Feature #72: Social Engineering Detection

## 1. `src/api/mod.rs` — Add module declaration

Add after `pub mod trust;`:

```rust
pub mod social_engineering;
```

## 2. `src/lib.rs` — Add routes to protected_api

Add these two routes inside the `protected_api` Router chain:

```rust
.route("/ai/detect-social-engineering", post(api::social_engineering::detect_social_engineering))
.route("/messages/{id}/social-engineering", get(api::social_engineering::get_social_engineering))
```

## 3. `src/db/migrations.rs` — Register migration 029

Add at the top with the other constants:

```rust
const MIGRATION_029: &str = include_str!("../../migrations/029_social_engineering.sql");
```

Add at the end of the `run()` function, before `Ok(())`:

```rust
if current_version < 29 {
    conn.execute_batch(MIGRATION_029)?;
    tracing::info!("Applied migration 029_social_engineering");
}
```

## 4. `web/src/lib/api.ts` — Add API methods

Add inside the `ai` section of the `api` object:

```typescript
socialEngineering: {
  detect: (messageId: string) =>
    request<{
      risk_level: 'none' | 'low' | 'medium' | 'high' | 'critical';
      tactics: { type: string; evidence: string; confidence: number }[];
      summary: string;
    }>('/api/ai/detect-social-engineering', {
      method: 'POST',
      body: JSON.stringify({ message_id: messageId }),
    }),
  get: (messageId: string) =>
    request<{
      risk_level: 'none' | 'low' | 'medium' | 'high' | 'critical';
      tactics: { type: string; evidence: string; confidence: number }[];
      summary: string;
    } | null>(`/api/messages/${messageId}/social-engineering`),
},
```

## 5. `web/src/pages/ThreadView.svelte` — Integrate banner (optional)

Add import at the top of the script:

```typescript
import SocialEngineeringBanner from '../components/thread/SocialEngineeringBanner.svelte';
```

Add state variable:

```typescript
let seResult = $state<any>(null);
```

In the `loadThread()` function, after loading messages, add:

```typescript
// Check social engineering for the first (root) message
if (thread.messages.length > 0) {
  try {
    const cached = await api.ai.socialEngineering.get(thread.messages[0].id);
    seResult = cached;
  } catch {
    // Ignore — not critical
  }
}
```

In the template, add the banner right before the `<!-- Messages -->` section:

```svelte
{#if seResult}
  <SocialEngineeringBanner result={seResult} />
{/if}
```
