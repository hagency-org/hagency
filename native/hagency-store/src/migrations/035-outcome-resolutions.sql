-- Operator inspection credentials are separate from original host stop proof.
CREATE TABLE IF NOT EXISTS outcome_inspections (
    id TEXT PRIMARY KEY,
    dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id) ON DELETE CASCADE,
    fence INTEGER NOT NULL CHECK(fence > 0),
    receipt_digest TEXT NOT NULL CHECK(length(receipt_digest) = 64),
    snapshot_digest TEXT NOT NULL CHECK(length(snapshot_digest) = 64),
    token_hash TEXT NOT NULL CHECK(length(token_hash) = 64),
    created_at INTEGER NOT NULL CHECK(created_at >= 0),
    expires_at INTEGER NOT NULL CHECK(expires_at > created_at),
    consumed_at INTEGER
) STRICT;
CREATE INDEX IF NOT EXISTS outcome_inspections_dispatch ON outcome_inspections(dispatch_id);
CREATE INDEX IF NOT EXISTS outcome_inspections_expiry ON outcome_inspections(expires_at) WHERE consumed_at IS NULL;
CREATE TABLE IF NOT EXISTS outcome_resolutions (
    request_id TEXT PRIMARY KEY,
    dispatch_id TEXT NOT NULL UNIQUE REFERENCES runner_dispatches(id) ON DELETE CASCADE,
    inspection_id TEXT NOT NULL REFERENCES outcome_inspections(id),
    request_digest TEXT NOT NULL CHECK(length(request_digest) = 64),
    action TEXT NOT NULL CHECK(action IN ('continue','accept_completed','keep_blocked')),
    response TEXT NOT NULL CHECK(json_valid(response)),
    resolved_at INTEGER NOT NULL CHECK(resolved_at >= 0)
) STRICT;
