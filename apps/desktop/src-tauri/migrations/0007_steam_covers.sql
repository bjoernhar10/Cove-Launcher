-- Steam games used the 32x32 store icon as cover, and a catalogue match without
-- artwork cleared it. Both now use Steam's portrait library artwork instead
-- (the image cache falls back to smaller artwork when it is missing).
UPDATE games
SET cover_url = 'https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/'
    || (SELECT e.store_game_id FROM library_entries e
        WHERE e.game_id = games.id AND e.source_id = 'steam')
    || '/library_600x900_2x.jpg'
WHERE (cover_url IS NULL OR cover_url = '' OR cover_url LIKE '%/steamcommunity/public/images/apps/%')
  AND id IN (
    SELECT game_id FROM library_entries
    WHERE source_id = 'steam' AND store_game_id IS NOT NULL AND store_game_id GLOB '[0-9]*'
      AND store_game_id NOT GLOB '*[^0-9]*'
  );
