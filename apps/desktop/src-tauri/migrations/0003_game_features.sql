-- Recommender features of catalogue games (JSON: themes, gameModes, perspectives, keywords); NULL if unknown.
ALTER TABLE games ADD COLUMN features TEXT;
