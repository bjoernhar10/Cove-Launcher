-- itch.io extras (soundtrack, artbook …) downloaded with a game: a separate
-- download of the same game that goes into the game's "Extras" folder and
-- never becomes a library entry of its own. NULL for the game itself.
ALTER TABLE downloads ADD COLUMN extra TEXT;
