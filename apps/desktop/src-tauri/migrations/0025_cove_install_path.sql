-- The folder Cove installed the game into. "Installed by Cove" used to come
-- only from the downloads table, so clearing the download history turned a
-- Cove install into a store-client install (Epic games then started through
-- the Epic Games Launcher). NULL = not installed by Cove.
ALTER TABLE library_entries ADD COLUMN cove_install_path TEXT;

UPDATE library_entries SET cove_install_path = install_path
WHERE install_path IS NOT NULL AND EXISTS (
  SELECT 1 FROM downloads d
  WHERE d.game_id = library_entries.game_id AND d.status = 'completed'
    AND d.install_path = library_entries.install_path
);
