# Wiring Instructions for Feature #48: Relationship-Aware Prioritization

## 1. `src/api/mod.rs` — Add module declaration

Add this line (alphabetical order, after `queue_status`):

```rust
pub mod relationship_priority;
```

## 2. `src/lib.rs` — Add routes to protected_api

Add these three routes inside the `protected_api` Router builder (after the existing `/ai/` routes):

```rust
.route("/ai/relationship-priority", post(api::relationship_priority::compute_relationship_scores))
.route("/contacts/{email}/relationship", get(api::relationship_priority::get_contact_relationship))
.route("/messages/prioritized", get(api::relationship_priority::get_prioritized_messages))
```

## 3. `src/db/migrations.rs` — Add migration 028

Add the const at the top with the other migrations:

```rust
const MIGRATION_028: &str = include_str!("../../migrations/028_relationship_priority.sql");
```

Add the migration block at the end of the `run` function (before `Ok(())`):

```rust
if current_version < 28 {
    conn.execute_batch(MIGRATION_028)?;
    tracing::info!("Applied migration 028_relationship_priority");
}
```

## 4. `web/src/lib/api.ts` — Add API methods

Add the `RelationshipScore` interface and API methods. Inside the `api` object, add a new namespace:

```typescript
// After auditLog namespace:
relationshipPriority: {
  compute: () => request<{ scored: number }>('/api/ai/relationship-priority', { method: 'POST' }),
  getForContact: (email: string) =>
    request<{
      email: string;
      score: number;
      frequency_score: number;
      recency_score: number;
      reply_rate_score: number;
      bidirectional_score: number;
      thread_depth_score: number;
      computed_at: number;
    }>(`/api/contacts/${encodeURIComponent(email)}/relationship`),
  getPrioritized: (params?: { account_id?: string; folder?: string; limit?: number; offset?: number }) => {
    const query = new URLSearchParams();
    if (params?.account_id) query.set('account_id', params.account_id);
    if (params?.folder) query.set('folder', params.folder);
    if (params?.limit) query.set('limit', String(params.limit));
    if (params?.offset) query.set('offset', String(params.offset));
    return request<{
      messages: Array<{
        id: string;
        account_id: string;
        thread_id?: string;
        folder: string;
        from_address?: string;
        from_name?: string;
        subject?: string;
        snippet?: string;
        date?: number;
        is_read: boolean;
        is_starred: boolean;
        has_attachments: boolean;
        labels?: string;
        ai_priority_label?: string;
        ai_category?: string;
        relationship_score: number;
        blended_score: number;
      }>;
      total: number;
    }>(`/api/messages/prioritized?${query}`);
  },
},
```

## 5. `src/models/message.rs` — No changes needed

The `MessageSummary` struct and `from_row` method are used as-is by the prioritized endpoint.
