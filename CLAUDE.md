# Iris

@AGENTS.md

## Claude cloud sessions

- GitHub GraphQL is blocked in cloud sessions, so `gh pr list` and `gh pr view`
  fail with HTTP 403. Use the REST API: `gh api repos/divyekant/iris/pulls`.
  `scripts/review/review.py` uses REST only.
- Do not set `GH_TOKEN` or `GITHUB_TOKEN`. The cloud GitHub proxy signs in `gh`
  and git.
- The SessionStart hook starts the local CI warm-up. Its log is
  `.local-ci/prepare.log`.
- Do not use `sleep` or a polling loop to wait for a PR. The review command waits
  for the checks that it needs.

## Design Standards (managed by Kalos)

These standards apply to the web client in `client/`. `.kalos.yaml` holds the tokens.

- Palette "Iris": iris violet on cool ink neutrals. Primary: #a394ff (dark), #5b47d6 (light)
- Semantic colors are set for each palette: dark success=#5fd38d, warning=#f2b84b, error=#f47c7c; light success=#15703a, warning=#b7750c, error=#c0262d; info=primary
- Font: Inter, system-ui, sans-serif
- Mono font: 'SF Mono', 'Cascadia Mono', 'Menlo', monospace
- Type scale: 1.25 from 16px base
- Spacing: 4px base unit, use multiples only
- Radii: none=0, sm=4, md=8, lg=12, full=9999
- Max 12 unique colors
- All colors must map to design tokens
- Max 3 font families
- All spacing must be multiples of base unit
- Min contrast ratio: 4.5 (WCAG AA) for text, primary and success; 3 for warning, error and info (lines and dots only)
- Require alt text on all images
- Active brand: dark
- 2 brand palettes configured (dark, light)
- Transitions: fast=120ms ease, normal=200ms ease
- Unread indicator uses the primary color
- Priority badges use semantic colors: urgent=error, high=warning, normal=success, low=text_faint
