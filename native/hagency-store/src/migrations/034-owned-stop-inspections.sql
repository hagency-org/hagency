-- Host observations only, not runtime output and not stop settlement.
-- One immutable receipt per original failed attempt. Unknown attempts retain
-- this with their other evidence; deleting an eligible parent cascades it.
CREATE TABLE IF NOT EXISTS owned_stop_inspections (
    dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id) ON DELETE CASCADE,
    fence INTEGER NOT NULL CHECK(fence > 0),
    digest TEXT NOT NULL CHECK(length(digest) = 64),
    config TEXT NOT NULL CHECK(json_valid(config)),
    observed_at INTEGER NOT NULL CHECK(observed_at >= 0),
    PRIMARY KEY(dispatch_id, fence)
) STRICT;
