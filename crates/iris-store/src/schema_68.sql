-- Migration 68 (issue 1026): the known name of a gate sender
-- (`iris_api::gate`, `known_as`) finds a person sender with the same
-- display name, with no case. This partial index makes that lookup an
-- index search, not a read of each sender row.
CREATE INDEX IF NOT EXISTS sender_person_name_nocase ON sender(name COLLATE NOCASE)
  WHERE kind = 'person';
