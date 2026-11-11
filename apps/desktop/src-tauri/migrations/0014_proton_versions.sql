-- The Proton build a game uses: NULL = the default from the settings,
-- "latest" = the newest GE-Proton, a GE-Proton release name, or "steam:<folder>"
-- for a Proton build Steam installed.
ALTER TABLE library_entries ADD COLUMN proton_version TEXT;

-- "Proton Experimental" is now one of the Proton builds Steam installed.
UPDATE library_entries
   SET compatibility_tool = 'proton', proton_version = 'steam:Proton - Experimental'
 WHERE compatibility_tool = 'proton-experimental';
