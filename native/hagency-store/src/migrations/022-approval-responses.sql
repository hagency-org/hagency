-- No rows are backfilled: historical native application is not router authority.
CREATE TABLE approval_responses (
 request_id TEXT PRIMARY KEY REFERENCES owner_approvals(id),
 context_id TEXT NOT NULL REFERENCES approval_contexts(id),
 capability_digest TEXT NOT NULL,
 decision_digest TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('authorized','response_may_send','write_accepted','outcome_unknown')),
 write_accepted INTEGER NOT NULL DEFAULT 0 CHECK(write_accepted IN (0,1)),
 authorized_at INTEGER NOT NULL,
 response_started_at INTEGER
) STRICT;
CREATE INDEX approval_responses_context ON approval_responses(context_id,state);
