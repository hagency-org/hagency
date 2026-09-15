-- Immutable original file metadata and distinct nonrearmable event custody.
-- Existing uploads gain no metadata or executable publication permission.
CREATE TABLE file_deliveries (
 id TEXT PRIMARY KEY,
 upload_id TEXT NOT NULL UNIQUE REFERENCES file_uploads(id),
 dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),
 call_id TEXT NOT NULL,
 request TEXT NOT NULL CHECK(json_valid(request) AND length(CAST(request AS BLOB)) <= 8192),
 request_hash TEXT NOT NULL,
 captured TEXT CHECK(captured IS NULL OR (json_valid(captured) AND length(CAST(captured AS BLOB)) <= 8192)),
 event_state TEXT NOT NULL CHECK(event_state IN ('pending','claimed','write_possible','delivered')),
 claim_fence INTEGER NOT NULL DEFAULT 0 CHECK(claim_fence >= 0),
 claim_hash TEXT,
 claim_until INTEGER,
 transaction_id TEXT NOT NULL UNIQUE,
 publication TEXT CHECK(publication IS NULL OR (json_valid(publication) AND length(CAST(publication AS BLOB)) <= 8192)),
 cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK(cancel_requested IN (0,1)),
 failure TEXT CHECK(failure IS NULL OR failure IN ('cancelled','source_refused','staging_refused','publication_refused')),
 acceptance TEXT CHECK(acceptance IS NULL OR (json_valid(acceptance) AND length(CAST(acceptance AS BLOB)) <= 8192)),
 created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL,
 UNIQUE(dispatch_id,call_id),
 CHECK((event_state='delivered')=(acceptance IS NOT NULL)),
 CHECK((event_state IN ('write_possible','delivered'))=(publication IS NOT NULL)),
 CHECK(event_state='pending' OR captured IS NOT NULL)
) STRICT;
