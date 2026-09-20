-- The frozen window splits into the request and the discussion around it. Rows
-- already frozen for an in-flight dispatch were all delivered in its payload,
-- so the default keeps them the request and their completion unchanged.
ALTER TABLE dispatch_inputs ADD COLUMN addressed INTEGER NOT NULL DEFAULT 1 CHECK(addressed IN (0,1));
-- How much of the window the agent actually read, in parts, never decreasing.
-- Completion consumes only what was read; the rest returns to the session.
CREATE TABLE dispatch_conversation_reads (
 dispatch_id TEXT PRIMARY KEY REFERENCES runner_dispatches(id) ON DELETE CASCADE,
 read_parts INTEGER NOT NULL CHECK(read_parts >= 0)
) STRICT;
