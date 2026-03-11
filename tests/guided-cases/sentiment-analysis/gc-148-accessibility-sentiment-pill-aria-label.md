# GC-148: Accessibility — Sentiment Pill Has aria-label

## Metadata
- **Type**: accessibility
- **Priority**: P2
- **Surface**: ui
- **Flow**: sentiment-analysis
- **Tags**: sentiment-analysis, ui, accessibility, aria, wcag
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions

### Environment
- App running at http://127.0.0.1:3000
- At least one email account synced

### Data
- At least one message with a non-null `ai_sentiment` value visible in the inbox

## Steps

1. Navigate to the inbox
   - **Target**: http://127.0.0.1:3000/#/
   - **Input**: n/a
   - **Expected**: Inbox renders with at least one MessageRow containing a sentiment pill

2. Inspect the sentiment pill element for accessible labeling
   - **Target**: The `<span>` element that renders the sentiment pill (e.g., the one containing "Positive", "Negative", "Neutral", or "Mixed")
   - **Input**: Browser DevTools → Elements panel; or axe / screen reader
   - **Expected**: The pill has a `title` attribute set to `"Sentiment: {Label}"` (e.g., `title="Sentiment: Positive"`). Note: the current implementation uses `title` rather than `aria-label`.

3. Evaluate the title attribute as an accessibility mechanism
   - **Target**: The pill `<span>`
   - **Input**: n/a
   - **Expected**: `title` provides a tooltip that mouse users can discover. However, `title` alone is insufficient for keyboard-only or screen reader users — `aria-label` is recommended for full WCAG 2.1 AA compliance. Document this as a known gap.

4. Verify contrast ratio of the pill text against its background
   - **Target**: Computed foreground color (`--iris-color-success`, `--iris-color-error`, `--iris-color-warning`, or `--iris-color-text-faint`) against the 12% tint background
   - **Input**: Browser DevTools → Computed styles; or a contrast checker tool
   - **Expected**: Contrast ratio >= 4.5:1 (WCAG AA minimum for normal text at 10px is technically 7:1 for AAA; record the actual ratio)

## Success Criteria
- [ ] Every sentiment pill has a `title` attribute with the format `"Sentiment: {Label}"`
- [ ] The `title` attribute value is grammatically correct and human-readable
- [ ] The contrast ratio of pill text against pill background is >= 3:1 (minimum for UI components per WCAG 2.1 SC 1.4.11)

## Failure Criteria
- `title` attribute is absent from any sentiment pill
- `title` value does not identify the sentiment (e.g., blank or generic)
- Contrast ratio falls below 3:1 for any sentiment color variant

## Known Gap
- The sentiment pill currently uses only `title` for accessible labeling. `aria-label` should be added to the `<span>` for screen reader support, as `title` is not reliably announced by all assistive technologies. Example fix: add `aria-label="Sentiment: {sentimentConfig[message.ai_sentiment].label}"` to the pill element in `web/src/components/inbox/MessageRow.svelte` (line 122–128).
