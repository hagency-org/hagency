CREATE TABLE dispatch_recovery_reports (
 dispatch_id TEXT PRIMARY KEY REFERENCES runner_dispatches(id),
 task_id TEXT NOT NULL REFERENCES canonical_tasks(id),
 execution_epoch INTEGER NOT NULL CHECK(execution_epoch>=0)
) STRICT;
-- Earlier native kernel recoveries had no explicit report grant. Migrate only
-- reports whose original attempt has a durable done receipt for the current
-- task epoch; current task state alone cannot prove which attempt completed it.
WITH RECURSIVE historical_reports(dispatch_id,task_id,execution_epoch) AS (
 SELECT next.id,t.id,json_extract(t.config,'$.execution_epoch')
 FROM dispatch_recoveries r
 JOIN runner_dispatches old ON old.id=r.original_id AND old.state='outcome_unknown'
 JOIN runner_dispatches next ON next.id=r.replacement_id AND next.task_id IS NULL AND next.session_id=old.session_id
 JOIN canonical_tasks t ON t.id=old.task_id AND t.session_id=old.session_id
 WHERE json_extract(t.config,'$.status')='done' AND EXISTS(
   SELECT 1 FROM task_operation_receipts receipt WHERE receipt.dispatch_id=old.id
   AND json_extract(receipt.response,'$.id')=t.id
   AND json_extract(receipt.response,'$.status')='done'
   AND json_extract(receipt.response,'$.execution_epoch')=json_extract(t.config,'$.execution_epoch'))
 UNION
 SELECT next.id,previous.task_id,previous.execution_epoch
 FROM historical_reports previous JOIN dispatch_recoveries r ON r.original_id=previous.dispatch_id
 JOIN runner_dispatches old ON old.id=r.original_id AND old.state='outcome_unknown'
 JOIN runner_dispatches next ON next.id=r.replacement_id AND next.task_id IS NULL AND next.session_id=old.session_id
)
INSERT INTO dispatch_recovery_reports SELECT * FROM historical_reports;
CREATE VIEW current_recovery_reports AS
SELECT r.dispatch_id,r.task_id,r.execution_epoch
FROM dispatch_recovery_reports r
JOIN runner_dispatches d ON d.id=r.dispatch_id
JOIN canonical_tasks t ON t.id=r.task_id AND t.session_id=d.session_id
JOIN dispatch_recoveries recovery ON recovery.replacement_id=d.id
JOIN runner_dispatches old ON old.id=recovery.original_id AND old.session_id=d.session_id
WHERE d.task_id IS NULL AND old.state='outcome_unknown'
 AND (old.task_id=r.task_id OR EXISTS(SELECT 1 FROM dispatch_recovery_reports previous
   WHERE previous.dispatch_id=old.id AND previous.task_id=r.task_id AND previous.execution_epoch=r.execution_epoch))
 AND json_extract(t.config,'$.status')='done'
 AND json_extract(t.config,'$.execution_epoch')=r.execution_epoch
 AND NOT EXISTS(SELECT 1 FROM task_intents i WHERE i.session_id=d.session_id
   AND (i.task_id<>r.task_id OR i.state<>'active'));
