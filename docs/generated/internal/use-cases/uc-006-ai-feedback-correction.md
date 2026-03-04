---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
use-case: ai-feedback-correction
slug: uc-006-ai-feedback-correction
---

# Use Case: AI Feedback Correction

## Summary

A user notices that the AI has misclassified an email (wrong category, priority, or intent) and corrects it. The correction is recorded and, when the same pattern repeats, automatically influences future AI classifications.

## Actors

- **User**: The person correcting the AI classification.
- **System**: The Iris backend (feedback storage, classification pipeline).

## Preconditions

- AI classification is enabled and has run on the message.
- The message has AI metadata (ai_category, ai_priority_label, or ai_intent populated).

## Flow: Correct a Classification

1. User views a message in the inbox and notices the category pill shows "Promotions" when the email is actually a "Primary" work email.
2. User clicks on the category pill, which opens a correction dropdown.
3. User selects "Primary" from the list of valid categories.
4. Frontend calls `PUT /api/messages/{id}/ai-feedback` with `{ "field": "category", "value": "Primary" }`.
5. Backend validates:
   - `field` is one of: category, priority_label, intent.
   - `value` is valid for the field (e.g., categories: Primary, Updates, Social, Promotions, Finance, Travel, Newsletters).
6. Backend reads the current value of the AI field from the message.
7. Backend updates the message's `ai_category` column to "Primary."
8. Backend inserts a record into the `ai_feedback` table: message_id, field="category", original_value="Promotions", corrected_value="Primary".
9. Frontend updates the category pill to show "Primary."

## Flow: Feedback Influences Future Classification

1. The same user (or the system) corrects the same pattern multiple times. For example, emails from a specific newsletter are repeatedly corrected from "Promotions" to "Updates" (2+ occurrences).
2. During the next email sync, the AI classification pipeline calls `build_feedback_context`.
3. `build_feedback_context` queries the ai_feedback table for patterns with count >= 2: `SELECT field, original_value, corrected_value, COUNT(*) ... HAVING cnt >= 2`.
4. The result is formatted as a system prompt suffix: `"- The user corrected category from \"Promotions\" to \"Updates\" (3 times)"`.
5. This suffix is appended to the classification system prompt.
6. The Ollama model receives the feedback context alongside the email content and adjusts its classification accordingly.
7. Future similar emails are more likely to be classified as "Updates."

## Flow: View Feedback Statistics

1. User navigates to Settings > AI.
2. Frontend calls `GET /api/ai/feedback-stats`.
3. Backend returns: total corrections count, corrections grouped by field, and the top 20 most common correction patterns.
4. User can see which corrections they have made most frequently and how the system is learning.

## Postconditions

- The message's AI classification field is updated to the corrected value.
- A feedback record is stored in the ai_feedback table.
- When the correction pattern reaches 2+ occurrences, it will influence future classifications.

## Error Scenarios

| Scenario | System Response |
|---|---|
| Invalid field name (e.g., "summary") | 400 Bad Request |
| Invalid value for field (e.g., category="Spam") | 400 Bad Request |
| Message not found | 404 Not Found |
| Message has no AI metadata yet | Correction still applies (overwriting null with the corrected value) |

## Related Features

- fh-007-ai-classification
