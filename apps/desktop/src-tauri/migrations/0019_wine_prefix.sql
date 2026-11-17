-- A Wine prefix a game brought along (imported from Heroic or Lutris) and the
-- Wine program it runs with; NULL = Cove's own prefix and `wine` from PATH.
ALTER TABLE library_entries ADD COLUMN wine_prefix TEXT;
ALTER TABLE library_entries ADD COLUMN wine_binary TEXT;
