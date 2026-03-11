# GC-140: Sentiment Pill Displays for Positive Sentiment

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: sentiment-analysis
- **Tags**: sentiment-analysis, ui, inbox, pill, positive
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions

### Environment
- App running at http://127.0.0.1:3000
- At least one email account synced

### Data
- At least one message has `ai_sentiment = "positive"` stored in the database
- The message is in the INBOX folder (not deleted, not snoozed)

## Steps

1. Navigate to the inbox
   - **Target**: http://127.0.0.1:3000/#/
   - **Input**: n/a
   - **Expected**: Inbox loads and displays a list of MessageRow components

2. Locate a message row with a positive sentiment pill
   - **Target**: A `<span>` element inside a MessageRow that contains the text "Positive"
   - **Input**: n/a
   - **Expected**: A pill labeled "Positive" is visible in the sender/category row of the message

3. Inspect the pill's visual style
   - **Target**: The "Positive" pill `<span>` element
   - **Input**: n/a
   - **Expected**: The pill's `color` CSS property resolves to `var(--iris-color-success)` (green); its `background` is a 12% tint of the same color

4. Inspect the pill's tooltip
   - **Target**: The `title` attribute on the "Positive" pill `<span>`
   - **Input**: n/a
   - **Expected**: `title="Sentiment: Positive"`

## Success Criteria
- [ ] A pill with text "Positive" appears in the inbox for messages with `ai_sentiment = "positive"`
- [ ] Pill foreground color resolves to the success token (green, `--iris-color-success`)
- [ ] Pill background is a semi-transparent tint of the success color
- [ ] `title` attribute reads "Sentiment: Positive"

## Failure Criteria
- No pill appears for positive-sentiment messages
- Pill text differs from "Positive"
- Pill color is not the success/green design token
- `title` attribute is missing or incorrect
