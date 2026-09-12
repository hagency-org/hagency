-- No table references task_notices; rebuild its state constraint atomically.
CREATE TABLE task_notices_v14 (
 id TEXT PRIMARY KEY,task_id TEXT NOT NULL REFERENCES canonical_tasks(id),
 config TEXT NOT NULL CHECK(json_valid(config)),
 state TEXT NOT NULL CHECK(state IN ('pending','claimed','sending','uncertain','delivered','failed','cancelled')),
 claim_hash TEXT,claim_until INTEGER,delivery TEXT CHECK(delivery IS NULL OR json_valid(delivery)),
 error_code TEXT,not_before INTEGER NOT NULL DEFAULT 0,
 verified_route TEXT CHECK(verified_route IS NULL OR json_valid(verified_route)),content_digest TEXT,
 send_fence INTEGER NOT NULL DEFAULT 0 CHECK(send_fence>=0),
 cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK(cancel_requested IN (0,1)),
 task_epoch INTEGER,source_event_id TEXT
) STRICT;
INSERT INTO task_notices_v14(id,task_id,config,state,claim_hash,claim_until,delivery,error_code,not_before,verified_route,content_digest,send_fence,cancel_requested)
 SELECT id,task_id,config,
 CASE WHEN verified_route IS NULL OR state='delivered' THEN state WHEN state='claimed' THEN 'uncertain' ELSE 'cancelled' END,
 CASE WHEN verified_route IS NULL THEN claim_hash END,
 CASE WHEN verified_route IS NULL THEN claim_until END,
 delivery,CASE WHEN verified_route IS NOT NULL AND state<>'delivered' THEN 'legacy_send_scope' ELSE error_code END,
 not_before,verified_route,content_digest,
 CASE WHEN verified_route IS NOT NULL AND state='claimed' THEN 1 ELSE 0 END,
 CASE WHEN verified_route IS NOT NULL AND state<>'delivered' THEN 1 ELSE 0 END
 FROM task_notices;
DROP TABLE task_notices;
ALTER TABLE task_notices_v14 RENAME TO task_notices;
CREATE INDEX task_notice_ready ON task_notices(state,not_before,id);
CREATE TABLE notice_send_inspections (
 notice_id TEXT NOT NULL REFERENCES task_notices(id),fence INTEGER NOT NULL,digest TEXT NOT NULL,
 observation TEXT NOT NULL CHECK(json_valid(observation)),
 PRIMARY KEY(notice_id,fence)
) STRICT;
