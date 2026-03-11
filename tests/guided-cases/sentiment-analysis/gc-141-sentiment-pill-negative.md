# GC-141: Sentiment Pill Displays for Negative Sentiment

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: sentiment-analysis
- **Tags**: sentiment-analysis, ui, inbox, pill, negative
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions

### Environment
- App running at http://127.0.0.1:3000
- At least one email account synced

### Data
- At least one message has `ai_sentiment = "negative"` stored in the database
- The message is in the INBOX folder (not deleted, not snoozed)

## Steps

1. Navigate to the inbox
   - **Target**: http://127.0.0.1:3000/#/
   - **Input**: n/a
   - **Expected**: Inbox loads and displays MessageRow components

2. Locate a message row with a negative sentiment pill
   - **Target**: A `<span>` element inside a MessageRow that contains the text "Negative"
   - **Input**: n/a
   - **Expected**: A pill labeled "Negative" is visible in the sender/category row

3. Inspect the pill's visual style
   - **Target**: The "Negative" pill `<span>` element
   - **Input**: n/a
   - **Expected**: The pill's `color` CSS property resolves to `var(--iris-color-error)` (red); its `background` is a 12% tint of the error color

4. Inspect the pill's tooltip
   - **Target**: The `title` attribute on the "Negative" pill `<span>`
   - **Input**: n/a
   - **Expected**: `title="Sentiment: Negative"`

## Success Criteria
- [ ] A pill with text "Negative" appears for messages with `ai_sentiment = "negative"`
- [ ] Pill foreground color resolves to the error token (red, `--iris-color-error`)
- [ ] Pill background is a semi-transparent tint of the error color
- [ ] `title` attribute reads "Sentiment: Negative"

## Failure Criteria
- No pill appears for negative-sentiment messages
- Pill text differs from "Negative"
- Pill color is not the error/red design token
- `title` attribute is missing or incorrect
