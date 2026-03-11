# GC-142: Sentiment Pill Displays for Mixed Sentiment

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: sentiment-analysis
- **Tags**: sentiment-analysis, ui, inbox, pill, mixed
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions

### Environment
- App running at http://127.0.0.1:3000
- At least one email account synced

### Data
- At least one message has `ai_sentiment = "mixed"` stored in the database
- The message is in the INBOX folder (not deleted, not snoozed)

## Steps

1. Navigate to the inbox
   - **Target**: http://127.0.0.1:3000/#/
   - **Input**: n/a
   - **Expected**: Inbox loads and displays MessageRow components

2. Locate a message row with a mixed sentiment pill
   - **Target**: A `<span>` element inside a MessageRow that contains the text "Mixed"
   - **Input**: n/a
   - **Expected**: A pill labeled "Mixed" is visible in the sender/category row

3. Inspect the pill's visual style
   - **Target**: The "Mixed" pill `<span>` element
   - **Input**: n/a
   - **Expected**: The pill's `color` CSS property resolves to `var(--iris-color-warning)` (yellow/amber); its `background` is a 12% tint of the warning color

4. Inspect the pill's tooltip
   - **Target**: The `title` attribute on the "Mixed" pill `<span>`
   - **Input**: n/a
   - **Expected**: `title="Sentiment: Mixed"`

5. Confirm "Neutral" pill is also rendered correctly for any neutral-sentiment message visible in the inbox
   - **Target**: A `<span>` containing "Neutral" if such a message is visible
   - **Input**: n/a
   - **Expected**: Neutral pill color resolves to `var(--iris-color-text-faint)` (gray)

## Success Criteria
- [ ] A pill with text "Mixed" appears for messages with `ai_sentiment = "mixed"`
- [ ] Pill foreground color resolves to the warning token (yellow/amber, `--iris-color-warning`)
- [ ] Pill background is a semi-transparent tint of the warning color
- [ ] `title` attribute reads "Sentiment: Mixed"
- [ ] Neutral pill (if visible) uses `--iris-color-text-faint` (gray)

## Failure Criteria
- No pill appears for mixed-sentiment messages
- Pill text differs from "Mixed"
- Pill color is not the warning/yellow design token
- `title` attribute is missing or incorrect
