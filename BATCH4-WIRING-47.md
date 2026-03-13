# Batch 4 Wiring — Feature #47 VIP Auto-Detection

## Routes to add in `src/lib.rs` (protected_api section)

Add these routes to the `protected_api` Router in `build_app()`:

```rust
.route("/contacts/vip", get(api::vip::list_vip).post(api::vip::compute_vip))  // overload: GET=list, but compute needs its own path
.route("/contacts/vip/compute", post(api::vip::compute_vip))
.route("/contacts/{email}/vip", put(api::vip::set_vip))
.route("/contacts/{email}/vip-score", get(api::vip::get_vip_score))
```

Exact lines to add after the existing routes (e.g., after the `/audit-log` route):

```rust
.route("/contacts/vip", get(api::vip::list_vip))
.route("/contacts/vip/compute", post(api::vip::compute_vip))
.route("/contacts/{email}/vip", put(api::vip::set_vip))
.route("/contacts/{email}/vip-score", get(api::vip::get_vip_score))
```

## Module registration

Already done — `pub mod vip;` added to `src/api/mod.rs`.

## Migration registration

Already done — migration 026 added to `src/db/migrations.rs`.

## Files created/modified

| File | Action |
|------|--------|
| `migrations/026_vip_contacts.sql` | Created — VIP contacts table + indexes |
| `src/api/vip.rs` | Created — 4 handlers + VIP score algorithm + 11 unit tests |
| `src/api/mod.rs` | Modified — added `pub mod vip;` |
| `src/db/migrations.rs` | Modified — added migration 026 include + runner |
| `web/src/components/inbox/VipBadge.svelte` | Created — gold crown badge with tooltip |
| `web/src/components/contacts/VipList.svelte` | Created — VIP contacts management list |
| `web/src/lib/api.ts` | Modified — added VipContact type + vip API namespace |

## Endpoints Summary

| Method | Path | Handler | Description |
|--------|------|---------|-------------|
| GET | `/api/contacts/vip` | `list_vip` | List VIP contacts (threshold + manual) |
| POST | `/api/contacts/vip/compute` | `compute_vip` | Recompute all VIP scores |
| PUT | `/api/contacts/{email}/vip` | `set_vip` | Manual VIP toggle |
| GET | `/api/contacts/{email}/vip-score` | `get_vip_score` | Single contact VIP score |
