-- KnowledgeBrain 0009: document attempt + description (spec §2.1 / §5.8).

ALTER TABLE documents ADD COLUMN IF NOT EXISTS description text NOT NULL DEFAULT '';
ALTER TABLE documents ADD COLUMN IF NOT EXISTS attempt integer NOT NULL DEFAULT 1;
