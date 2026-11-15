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

## D1 (2026-11-15): Iris keeps its own tracker as the board

- Decided by: dk
- Why: The Iris tracker holds the 94 work items of the seven releases, with
  their release, part and done condition. A move to the kit board adds risk and
  gains little now.
- Options not chosen: Publish the kit board and copy the 94 items to it.
- Links: https://claude.ai/artifact/VWqsmpqHNgiop14TpoecjV
