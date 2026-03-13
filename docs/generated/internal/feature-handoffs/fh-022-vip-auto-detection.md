---
id: fh-022
type: feature-handoff
audience: internal
topic: VIP Auto-Detection
status: draft
generated: 2026-03-13
source-tier: direct
context-files: [src/api/vip.rs, web/src/components/VipBadge.svelte, web/src/components/VipList.svelte, migrations/026_vip_contacts.sql]
hermes-version: 1.0.0
---

# Feature Handoff: VIP Auto-Detection

## What It Does

VIP Auto-Detection computes a numeric importance score for each contact based on observed email behavior — how frequently they exchange messages with the user, how reliably the user replies to them, how recently they have been active, and how deeply threaded their conversations are. Contacts that score above a configurable threshold are automatically classified as VIPs.

Manual VIP designation is also supported: any contact can be pinned as a VIP regardless of computed score. Manual VIPs always receive a score of 1.0 and are not affected by recomputation. The feature surfaces VIPs through a `VipBadge` component (a gold crown rendered in inbox rows and thread views) and a `VipList` component that enumerates all VIP contacts with their scores.

## How It Works

**Endpoints**:
- `POST /api/contacts/vip/compute` — recompute VIP scores for all contacts
- `GET /api/contacts/vip` — list contacts classified as VIP
- `PUT /api/contacts/{email}/vip` — manually toggle VIP status for a contact
- `GET /api/contacts/{email}/vip-score` — retrieve the VIP score and component breakdown for a contact

---

**POST /api/contacts/vip/compute**

Triggers a full recomputation of VIP scores across all contacts. This is a potentially expensive operation on large mailboxes; it is designed to be run periodically (e.g., once per sync cycle or on demand) rather than after every message.

Processing sequence for each contact:
1. Computes four component scores from the `messages` table and existing `vip_contacts` data:

   - **frequency** (weight 0.3): Number of messages exchanged with the contact over the past 90 days, normalized against the maximum frequency across all contacts. A contact with the most messages gets frequency=1.0; others scale proportionally.
   - **reply_rate** (weight 0.25): Fraction of messages from this contact to which the user replied, over the past 90 days. Computed as `user_replies_to_contact / contact_messages_to_user`. If the contact has sent no messages, reply_rate=0.
   - **recency_decay** (weight 0.25): Exponential decay based on days since last message. Contacts active within the past 7 days receive recency≈1.0. The decay constant is set so that 30 days of inactivity yields approximately 0.5 and 90 days yields approximately 0.1.
   - **thread_depth** (weight 0.2): Average number of messages per thread involving the contact, normalized against a cap of 10 messages per thread (threads deeper than 10 are treated as 10). This rewards sustained back-and-forth conversations.

2. Final score = `0.3 × frequency + 0.25 × reply_rate + 0.25 × recency_decay + 0.2 × thread_depth`.

3. Scores are clamped to [0.0, 1.0].

4. For contacts with `manual_vip = true`, the computed score is stored for reference but the effective score shown to users is always 1.0 and the `is_vip` flag is not affected by the threshold.

5. For non-manual contacts, `is_vip` is set to true if computed score ≥ 0.6 (the default threshold). The threshold applies only to computed classification; manual VIPs bypass it.

6. Upserts rows in `vip_contacts` for all processed contacts.

Response:
```json
{
  "recomputed": 142,
  "vip_count": 8
}
```

---

**GET /api/contacts/vip**

Query parameters:

| Parameter | Type | Default | Description |
|---|---|---|---|
| `threshold` | float | 0.6 | Minimum score to include in results (applies to computed scores only; manual VIPs always included) |

Returns contacts where `is_vip = true` or `manual_vip = true`, filtered to those meeting the threshold, ordered by score descending.

Response per contact:
```json
{
  "email": "sarah@example.com",
  "display_name": "Sarah Chen",
  "score": 0.87,
  "manual_vip": false,
  "frequency_score": 0.92,
  "reply_rate_score": 0.80,
  "recency_score": 0.95,
  "thread_depth_score": 0.70
}
```

---

**PUT /api/contacts/{email}/vip**

Request body:
```json
{
  "manual_vip": true
}
```

Sets `manual_vip` for the contact. If `manual_vip: true`, the contact's effective score is set to 1.0 and `is_vip` is set to true immediately, without requiring a recompute. If `manual_vip: false`, the contact reverts to computed scoring on the next recompute; `is_vip` is not cleared immediately (it reflects the last computed result until recompute runs).

Returns 404 if the contact email has never appeared in the `messages` table.

---

**GET /api/contacts/{email}/vip-score**

Returns the full score breakdown for a single contact. Returns the contact's current stored values; does not trigger recomputation. Returns 404 if no record exists in `vip_contacts` for the contact (recompute has not been run, or the contact has no messages in the DB).

---

**Migration 026 — `vip_contacts` table**:

| Column | Type | Notes |
|---|---|---|
| `email` | TEXT PRIMARY KEY | Contact email address |
| `display_name` | TEXT | Most recently seen display name |
| `score` | REAL NOT NULL DEFAULT 0.0 | Computed VIP score (0.0–1.0) |
| `is_vip` | INTEGER NOT NULL DEFAULT 0 | Boolean — true if score ≥ threshold or manual_vip |
| `manual_vip` | INTEGER NOT NULL DEFAULT 0 | Boolean — user-set manual override |
| `frequency_score` | REAL | Component breakdown |
| `reply_rate_score` | REAL | Component breakdown |
| `recency_score` | REAL | Component breakdown |
| `thread_depth_score` | REAL | Component breakdown |
| `last_computed_at` | TEXT | ISO 8601 timestamp of last recompute |

**Implementation files**:
- `src/api/vip.rs` — route handlers, score computation logic
- `migrations/026_vip_contacts.sql` — schema migration
- `web/src/components/VipBadge.svelte` — gold crown badge
- `web/src/components/VipList.svelte` — full VIP contact list

## User-Facing Behavior

**VipBadge** renders as a small gold crown icon next to the sender name in inbox rows and thread view headers. It appears whenever `is_vip = true` for the sender's email address. The badge has no click behavior — it is display-only.

**VipList** renders in the Settings or Contacts section and shows all VIP contacts with their scores and component breakdowns. Each row includes a manual VIP toggle (star or pin icon). Contacts with `manual_vip = true` display a distinct visual indicator (e.g., filled vs. outlined crown) to distinguish manual from auto-detected VIPs.

**Score breakdown display**: The `VipList` component optionally expands each contact row to show the four component scores (frequency, reply rate, recency, thread depth) as a mini progress bar or percentage breakdown. This aids user understanding of why a contact was auto-detected as VIP.

**Recompute trigger**: Recomputation is triggered via a button in the VipList UI ("Refresh VIP scores") or programmatically via POST /api/contacts/vip/compute. There is no automatic scheduling in the initial implementation.

## Configuration

No additional configuration required. The scoring weights (0.3, 0.25, 0.25, 0.2) and threshold (0.6) are hardcoded. The recency decay constant is hardcoded to produce a half-life of approximately 30 days.

| Parameter | Value | Location |
|---|---|---|
| Default VIP threshold | 0.6 | `src/api/vip.rs` — `VIP_THRESHOLD` constant |
| Frequency weight | 0.3 | `src/api/vip.rs` |
| Reply rate weight | 0.25 | `src/api/vip.rs` |
| Recency decay weight | 0.25 | `src/api/vip.rs` |
| Thread depth weight | 0.2 | `src/api/vip.rs` |
| Recency half-life | ~30 days | `src/api/vip.rs` — decay constant |
| Thread depth cap | 10 messages | `src/api/vip.rs` |

The `?threshold` query parameter on GET /api/contacts/vip allows per-request adjustment without changing the stored data.

## Edge Cases & Limitations

- **Manual VIP persists across recomputes**: Setting `manual_vip = true` on a contact means that contact will always appear in VIP lists and always have a score of 1.0, regardless of email activity. Setting `manual_vip = false` reverts them to computed scoring, but their `is_vip` flag is not updated until the next recompute.
- **Frequency normalization is relative**: The frequency score is normalized against the contact with the most messages in the past 90 days. If one contact has an unusually high message volume, all others score proportionally lower. A contact with moderate but consistent communication may score low on frequency despite being genuinely important.
- **No AI involved**: Scoring is entirely algorithmic — SQL queries and arithmetic. There is no AI provider dependency for this feature.
- **Score data goes stale**: Scores are computed at recompute time and stored. They do not update in real time as new messages arrive. A contact whose frequency drops after a recompute retains their old (potentially higher) score until the next recompute.
- **Reply rate direction**: Reply rate measures the user replying to the contact, not the contact replying to the user. A contact who sends many emails and receives few replies from the user will have a low reply rate score, which is intentional — it reflects the user's own engagement level.
- **90-day lookback window**: All four components use message data from the past 90 days. Contacts with no recent activity will have low scores regardless of historical communication volume. There is no "long-term loyalty" component.
- **No cross-account deduplication**: VIP scoring runs per-email-address. If the same person contacts the user from multiple addresses, those are treated as separate contacts with separate scores. A user may want both addresses to be VIP but must manually VIP each one.
- **`vip_contacts` table starts empty**: Migration 026 creates the table but does not backfill. VIP data does not exist until POST /api/contacts/vip/compute is run for the first time.

## Common Questions

**Q: A contact I email constantly is not showing as VIP — why?**
Check the component scores via GET /api/contacts/{email}/vip-score. A high-frequency contact may have a low recency score (no recent exchanges) or low reply rate (user rarely replies), pulling the composite score below 0.6. Frequency alone (weight 0.3) cannot exceed 0.3, which is below the 0.6 threshold. Additionally, if the normalization denominator is dominated by one very high-volume contact, others may score low even with significant activity. Use the manual VIP toggle as an override.

**Q: How long does recomputation take?**
Depends on mailbox size. The query joins across all messages to compute per-contact aggregates in SQL. For mailboxes with tens of thousands of messages and hundreds of contacts, expect a few seconds. For very large mailboxes, consider running recompute at low-traffic times or off the main request path via the job queue.

**Q: If I unset manual_vip, when does the contact's VipBadge disappear?**
The badge reflects `is_vip` from the `vip_contacts` table. Setting `manual_vip = false` does not immediately update `is_vip` — it remains as set by the last recompute. If the last recompute classified the contact as VIP, the badge persists until the next recompute. To immediately remove the badge, run POST /api/contacts/vip/compute after unsetting the manual flag.

**Q: Does VIP status affect inbox sorting or priority?**
Not in the current implementation. VIP status is a display signal (badge) and a filtering mechanism (VipList), but it does not reorder the inbox or boost message priority. Integration with inbox sorting is a future enhancement.

**Q: What does the gold crown look like in the light brand palette?**
The `VipBadge` uses the brand gold token (`--iris-color-brand-gold`), which is defined in both the dark and light brand palettes in `.kalos.yaml`. The crown renders consistently across both themes.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| GET /api/contacts/vip returns empty list | Recompute has never been run | Run POST /api/contacts/vip/compute first; `vip_contacts` table is empty until then |
| Contact expected to be VIP is not shown | Score below threshold | Check GET /api/contacts/{email}/vip-score for component breakdown; use manual VIP if needed |
| GET /api/contacts/{email}/vip-score returns 404 | Contact not in `vip_contacts` (recompute not run, or email not in messages table) | Run recompute; verify contact email appears in synced messages |
| Score is 1.0 for unexpected contact | Contact has `manual_vip = true` | Check VipList for the manual pin indicator; use PUT /api/contacts/{email}/vip with `manual_vip: false` to revert |
| VipBadge shows after unsetting manual VIP | `is_vip` not updated until next recompute | Run POST /api/contacts/vip/compute to refresh |
| Recompute returns 500 | DB error during aggregation query | Check `iris-server` logs; verify `messages` table is not corrupted |
| Frequency scores all look low | One very high-volume contact dominating normalization | Expected — normalization is relative; use manual VIP for important contacts with moderate volume |

## Related Links

- Backend: `src/api/vip.rs`
- Migration: `migrations/026_vip_contacts.sql`
- Frontend: `web/src/components/VipBadge.svelte`, `web/src/components/VipList.svelte`
- Related: FH-018 (Key Topics — contacts namespace), FH-019 (Response Time Patterns — contacts namespace), FH-013 (Job Queue — periodic recompute integration path)
- Design tokens: `web/src/tokens.css` (`--iris-color-brand-gold` for crown color)
