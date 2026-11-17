-- Migration 32: the commitment of a nudge (card R3-07). A nudge is a reply
-- draft with the fixed nudge intent of iris-voice. `ask` is the commitment
-- that the model got (title, due and quote, as JSON text), so a nudge is
-- ready only while its commitment is the same. NULL for other drafts.

ALTER TABLE reply_draft ADD COLUMN ask TEXT;
