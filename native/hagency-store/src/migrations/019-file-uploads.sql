-- Secrets and transport receipt identities remain in the protected domain DB.
-- Possible POSTs are never reset by restart, lease expiry or cancellation.
CREATE TABLE file_uploads (
 id TEXT PRIMARY KEY,
 dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),
 call_id TEXT NOT NULL,
 request_digest TEXT NOT NULL,
 capability_digest TEXT NOT NULL,
 scope_fingerprint TEXT NOT NULL,
 route TEXT NOT NULL CHECK(json_valid(route)),
 preparation_hash TEXT NOT NULL,
 stage TEXT CHECK(stage IS NULL OR json_valid(stage)),
 stage_state TEXT NOT NULL CHECK(stage_state IN ('unbound','bound','staged','unknown')),
 upload_state TEXT NOT NULL CHECK(upload_state IN ('pending','claimed','write_possible','accepted')),
 claim_fence INTEGER NOT NULL DEFAULT 0 CHECK(claim_fence >= 0),
 claim_hash TEXT,
 claim_until INTEGER,
 cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK(cancel_requested IN (0,1)),
 outcome_unknown INTEGER NOT NULL DEFAULT 0 CHECK(outcome_unknown IN (0,1)),
 acceptance TEXT CHECK(acceptance IS NULL OR json_valid(acceptance)),
 created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL,
 UNIQUE(dispatch_id,call_id),
 CHECK((stage_state='unbound')=(stage IS NULL)),
 CHECK((upload_state='accepted')=(acceptance IS NOT NULL)),
 CHECK(upload_state NOT IN ('claimed','write_possible','accepted') OR stage_state='staged')
) STRICT;
CREATE UNIQUE INDEX file_upload_stage_identity ON file_uploads(json_extract(stage,'$.namespace_digest'),json_extract(stage,'$.operation_id')) WHERE stage IS NOT NULL;
