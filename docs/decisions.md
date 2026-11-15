# Decisions

One entry for each decision that changes the product or how it is built: a
direction, a tool, a data model, a rule. Put the newest entry at the top. Do not
edit an old entry. If a decision changes, add a new entry that names the old one.

## Template

```markdown
## D<n> (<YYYY-MM-DD>): <the decision in a few words>

- Decided by: <name>
- Why: <the reason, in one or two sentences>
- Options not chosen: <each option, and why not>
- Links: <card, spec, PR, before/after page>
```

## D2 (2026-11-15): Each user signs in to Gmail with an own Google OAuth client

- Decided by: dk
- Why: Google needs no app verification for personal use with fewer than
  100 users. The path costs nothing, keeps the read-only scope
  `gmail.readonly`, and keeps spec 1. The client must be "In production",
  because "Testing" tokens expire after 7 days. The domain test of the record
  is a condition.
- Options not chosen: A verified Iris app (weeks of review, a yearly security
  assessment, a public client secret). IMAP with an app password (full mail
  access, no read-only option, new sync design). IMAP with OAuth, the macOS
  Mail app, Google Takeout and forwarding.
- Links: card R1-01, `docs/decisions/2026-11-google-sign-in.md`, PR #13

## D1 (2026-11-15): Iris keeps its own tracker as the board

- Decided by: dk
- Why: The Iris tracker holds the 94 work items of the seven releases, with
  their release, part and done condition. A move to the kit board adds risk and
  gains little now.
- Options not chosen: Publish the kit board and copy the 94 items to it.
- Links: https://claude.ai/artifact/VWqsmpqHNgiop14TpoecjV
