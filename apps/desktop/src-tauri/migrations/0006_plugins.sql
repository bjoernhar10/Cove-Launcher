-- Step 7: plugin permissions, per-plugin storage and plugin achievements.

-- Approval state. A plugin without a row (or whose manifest asks for more than
-- `granted`) waits for the user's confirmation.
CREATE TABLE plugins (
    id               TEXT PRIMARY KEY,
    enabled          INTEGER NOT NULL DEFAULT 1,
    granted          TEXT NOT NULL DEFAULT '[]',
    approved_version TEXT,
    approved_at      TEXT NOT NULL
);

-- `storage` permission: small JSON values per plugin.
CREATE TABLE plugin_storage (
    plugin_id TEXT NOT NULL,
    key       TEXT NOT NULL,
    value     TEXT NOT NULL,
    PRIMARY KEY (plugin_id, key)
);

-- `achievements.define`: the plugin's current definitions (JSON array).
CREATE TABLE plugin_achievements (
    plugin_id   TEXT PRIMARY KEY,
    definitions TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- The old on/off map (no permissions) is replaced by the table above.
DELETE FROM settings WHERE key = 'plugins';
