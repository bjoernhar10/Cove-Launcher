-- Steam games get the landscape header for wide views ("Weiterspielen");
-- until now only the portrait cover was set. The image cache resolves the
-- hashed header path of newer games through the Steam store API.
UPDATE games
SET header_url = 'https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/'
    || (SELECT e.store_game_id FROM library_entries e
        WHERE e.game_id = games.id AND e.source_id = 'steam')
    || '/header.jpg'
WHERE (header_url IS NULL OR header_url = '')
  AND id IN (
    SELECT game_id FROM library_entries
    WHERE source_id = 'steam' AND store_game_id GLOB '[0-9]*'
      AND store_game_id NOT GLOB '*[^0-9]*'
  );
