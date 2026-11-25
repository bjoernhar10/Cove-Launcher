-- Store games Cove installed whose download entry was already cleared from the
-- history: store_installs (written only by Cove's GOG, Epic and Amazon
-- installs, removed on uninstall) still knows them.
UPDATE library_entries SET cove_install_path = install_path
WHERE cove_install_path IS NULL AND installed = 1 AND install_path IS NOT NULL
  AND EXISTS (SELECT 1 FROM store_installs s WHERE s.game_id = library_entries.game_id);
