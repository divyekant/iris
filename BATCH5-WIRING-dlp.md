# Wiring Instructions for DLP Feature (#75)

## 1. `src/api/mod.rs` — ALREADY DONE

`pub mod dlp;` has been added (required for compilation and tests).

## 2. `src/lib.rs` — Add routes

Add these two routes inside the `protected_api` block, after the `/drafts/{id}` route:

```rust
.route("/compose/scan-dlp", post(api::dlp::scan_dlp))
.route("/compose/dlp-override", post(api::dlp::dlp_override))
```

## 3. `web/src/lib/api.ts` — Add DLP API methods

Add inside the `api` object (e.g., after the `drafts` block):

```typescript
dlp: {
  scan: (params: { subject: string; body: string; to: string[] }) =>
    request<{ findings: { type: string; match: string; location: string; line: number }[]; risk_level: string; allow_send: boolean }>('/api/compose/scan-dlp', {
      method: 'POST',
      body: JSON.stringify(params),
    }),
  override: () =>
    request<{ token: string }>('/api/compose/dlp-override', {
      method: 'POST',
      body: JSON.stringify({ findings_acknowledged: true }),
    }),
},
```

## 4. `Cargo.toml` — ALREADY DONE

`regex = "1"` and `once_cell = "1"` have been added.
