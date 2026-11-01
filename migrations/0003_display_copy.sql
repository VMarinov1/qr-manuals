-- Намалено JPEG копие на снимка за показване (NULL: показва се оригиналът).
ALTER TABLE files ADD COLUMN display_name TEXT;
ALTER TABLE files ADD COLUMN display_size INTEGER;
-- SQLite не позволява UNIQUE в ADD COLUMN, затова е отделен индекс.
CREATE UNIQUE INDEX files_display_name ON files (display_name);
