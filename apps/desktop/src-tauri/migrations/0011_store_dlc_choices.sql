-- DLCs the user left out in the install dialog (GOG product id or Epic app
-- name). Updates respect the choice; DLCs bought later are installed. Kept
-- after an uninstall so a reinstall offers the same choice.
CREATE TABLE store_dlc_choices (
    game_id TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    dlc_id  TEXT NOT NULL,
    PRIMARY KEY (game_id, dlc_id)
);
