-- Completion/send custody only. canonical_tasks remains the single task truth.
CREATE TABLE owned_task_completions (
 id TEXT PRIMARY KEY,
 dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id), fence INTEGER NOT NULL,
 task_id TEXT NOT NULL REFERENCES canonical_tasks(id), execution_epoch INTEGER NOT NULL,
 fingerprint TEXT NOT NULL, call_id TEXT NOT NULL, digest TEXT NOT NULL,
 body TEXT NOT NULL, route TEXT NOT NULL CHECK(json_valid(route)),
 deadline INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('held','ready','cancelled')),
 reply_id TEXT REFERENCES final_replies(id),
 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
 UNIQUE(dispatch_id,fence), UNIQUE(task_id,execution_epoch),
 FOREIGN KEY(dispatch_id,call_id) REFERENCES task_operation_receipts(dispatch_id,call_id),
 CHECK((state='ready')=(reply_id IS NOT NULL))
) STRICT;
