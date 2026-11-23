-- Games from Ubisoft Connect, Battle.net and the Xbox app start through their
-- launcher's link (uplay://, battlenet://, shell:AppsFolder\...).
-- NULL = the executable.
ALTER TABLE library_entries ADD COLUMN launch_url TEXT;
