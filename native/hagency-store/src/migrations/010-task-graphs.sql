CREATE TABLE task_graphs (
 id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL REFERENCES internal_conversations(id),
 creator_session_id TEXT NOT NULL REFERENCES runner_sessions(id),
 creator_dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),
 parent_task_id TEXT REFERENCES canonical_tasks(id),
 fleet_id TEXT NOT NULL,project_id TEXT NOT NULL,generation INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('active','complete','failed','cancelled')),
 config TEXT NOT NULL CHECK(json_valid(config)),created_at INTEGER NOT NULL,
 FOREIGN KEY(fleet_id,project_id) REFERENCES projects(fleet_id,id)
) STRICT;
CREATE INDEX graphs_creator ON task_graphs(creator_session_id,id);
CREATE TABLE graph_nodes (
 graph_id TEXT NOT NULL REFERENCES task_graphs(id),node_id TEXT NOT NULL,
 session_id TEXT NOT NULL REFERENCES runner_sessions(id),
 task_id TEXT NOT NULL UNIQUE REFERENCES canonical_tasks(id),
 message_sequence INTEGER UNIQUE REFERENCES peer_messages(sequence),
 state TEXT NOT NULL CHECK(state IN ('pending','dispatched','active','complete','failed','skipped','cancelled')),
 completed_epoch INTEGER,result_digest TEXT,result_receipt TEXT CHECK(result_receipt IS NULL OR json_valid(result_receipt)),
 result_value TEXT CHECK(result_value IS NULL OR json_valid(result_value)),
 PRIMARY KEY(graph_id,node_id),
 CHECK(state<>'complete' OR (completed_epoch IS NOT NULL AND result_digest IS NOT NULL AND result_receipt IS NOT NULL AND result_value IS NOT NULL))
) STRICT;
CREATE INDEX graph_node_session ON graph_nodes(session_id,graph_id);
CREATE TABLE graph_commands (
 dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),call_id TEXT NOT NULL,
 digest TEXT NOT NULL,response TEXT NOT NULL CHECK(json_valid(response)),
 PRIMARY KEY(dispatch_id,call_id)
) STRICT;
CREATE TABLE graph_dependencies (
 graph_id TEXT NOT NULL,node_id TEXT NOT NULL,sequence INTEGER NOT NULL,
 dependency_id TEXT NOT NULL,task_id TEXT NOT NULL REFERENCES canonical_tasks(id),
 execution_epoch INTEGER NOT NULL,digest TEXT NOT NULL,
 PRIMARY KEY(graph_id,node_id,sequence),UNIQUE(graph_id,node_id,dependency_id),
 FOREIGN KEY(graph_id,node_id) REFERENCES graph_nodes(graph_id,node_id),
 FOREIGN KEY(graph_id,dependency_id) REFERENCES graph_nodes(graph_id,node_id)
) STRICT;
-- Authorization for a finite stored definition does not depend on its creator
-- process staying alive. Quarantine is not a new runtime capability.
CREATE VIEW current_graph_scopes AS
SELECT g.id FROM task_graphs g
JOIN internal_conversations c ON c.id=g.conversation_id AND c.state='active'
JOIN runner_sessions owner ON owner.id=g.creator_session_id
JOIN engagements e ON e.id=owner.engagement_id AND e.state='active'
JOIN registrations r ON r.fleet_id=e.fleet_id AND r.generation=e.generation
WHERE g.fleet_id=e.fleet_id AND g.project_id=e.project_id AND g.generation=e.generation
 AND c.fleet_id=g.fleet_id AND c.project_id=g.project_id AND c.generation=g.generation
 AND (c.creator_session_id=owner.id OR EXISTS(SELECT 1 FROM internal_participants p
   WHERE p.conversation_id=c.id AND p.session_id=owner.id))
 AND (json_extract(owner.binding,'$.kind') IS NULL OR EXISTS(
   SELECT 1 FROM internal_participants p JOIN internal_conversations parent ON parent.id=p.conversation_id
   WHERE p.session_id=owner.id AND p.engagement_id=e.id AND parent.state='active'
   AND parent.fleet_id=e.fleet_id AND parent.project_id=e.project_id AND parent.generation=e.generation
   AND p.conversation_id=json_extract(owner.binding,'$.conversation_id')))
 AND NOT EXISTS(SELECT 1 FROM (SELECT DISTINCT session_id FROM graph_nodes WHERE graph_id=g.id) n WHERE NOT EXISTS(
   SELECT 1 FROM internal_participants p JOIN runner_sessions s ON s.id=p.session_id
   JOIN engagements a ON a.id=s.engagement_id
   WHERE p.conversation_id=c.id AND p.session_id=n.session_id AND p.engagement_id=a.id
   AND json_extract(s.binding,'$.kind')='internal' AND json_extract(s.binding,'$.conversation_id')=c.id
   AND a.state='active' AND a.fleet_id=g.fleet_id AND a.project_id=g.project_id AND a.generation=g.generation));

DROP VIEW live_peer_inputs;
CREATE VIEW conversation_peer_inputs AS
SELECT i.session_id,i.message_sequence
FROM peer_session_inputs i
JOIN peer_messages m ON m.sequence=i.message_sequence
JOIN internal_conversations c ON c.id=m.conversation_id
JOIN runner_sessions s ON s.id=i.session_id
JOIN engagements e ON e.id=s.engagement_id
JOIN registrations r ON r.fleet_id=e.fleet_id
WHERE c.state='active' AND e.state='active' AND e.generation=r.generation
 AND c.fleet_id=e.fleet_id AND c.project_id=e.project_id AND c.generation=e.generation
 AND (c.creator_session_id=s.id OR EXISTS(
   SELECT 1 FROM internal_participants p WHERE p.conversation_id=c.id AND p.session_id=s.id));
CREATE VIEW live_peer_inputs AS
SELECT i.session_id,i.message_sequence FROM conversation_peer_inputs i
WHERE NOT EXISTS(SELECT 1 FROM graph_nodes n WHERE n.message_sequence=i.message_sequence)
 OR EXISTS(SELECT 1 FROM graph_nodes n JOIN task_graphs g ON g.id=n.graph_id
 JOIN current_graph_scopes a ON a.id=g.id
 WHERE n.message_sequence=i.message_sequence AND n.session_id=i.session_id
 AND n.state IN ('dispatched','active') AND g.state='active');
CREATE VIEW graph_dispatch_ready AS
SELECT d.id AS dispatch_id FROM runner_dispatches d
JOIN graph_nodes n ON n.task_id=d.task_id AND n.session_id=d.session_id
JOIN task_graphs g ON g.id=n.graph_id AND g.state='active'
JOIN current_graph_scopes a ON a.id=g.id
JOIN peer_dispatch_inputs pi ON pi.dispatch_id=d.id AND pi.message_sequence=n.message_sequence
JOIN peer_session_inputs si ON si.session_id=d.session_id AND si.message_sequence=pi.message_sequence
WHERE n.state IN ('dispatched','active') AND si.dispatch_id=d.id AND si.processed_at IS NULL;
CREATE VIEW graph_dispatch_scope AS
SELECT d.id AS dispatch_id FROM runner_dispatches d
LEFT JOIN dispatch_recovery_reports report ON report.dispatch_id=d.id
JOIN canonical_tasks t ON t.id=COALESCE(d.task_id,report.task_id)
JOIN graph_nodes n ON n.task_id=t.id AND n.session_id=d.session_id
JOIN task_graphs g ON g.id=n.graph_id
JOIN current_graph_scopes a ON a.id=g.id
JOIN peer_dispatch_inputs pi ON pi.dispatch_id=d.id AND pi.message_sequence=n.message_sequence
JOIN peer_session_inputs si ON si.session_id=d.session_id AND si.message_sequence=pi.message_sequence
WHERE si.dispatch_id=d.id AND si.processed_at IS NULL
 AND ((g.state='active' AND n.state IN ('dispatched','active'))
 OR (n.state='complete' AND json_extract(t.config,'$.status')='done'
 AND n.completed_epoch=json_extract(t.config,'$.execution_epoch')));
CREATE VIEW admissible_dispatch_peer_inputs AS
SELECT pi.dispatch_id,pi.message_sequence FROM peer_dispatch_inputs pi
JOIN runner_dispatches d ON d.id=pi.dispatch_id
WHERE EXISTS(SELECT 1 FROM live_peer_inputs i WHERE i.session_id=d.session_id AND i.message_sequence=pi.message_sequence)
 OR EXISTS(SELECT 1 FROM current_recovery_reports r JOIN graph_nodes n ON n.task_id=r.task_id
 JOIN graph_dispatch_scope s ON s.dispatch_id=r.dispatch_id
 WHERE r.dispatch_id=d.id AND n.message_sequence=pi.message_sequence);

-- Earlier native versions dropped even shared leases on outcome_unknown. The
-- host-owned resource declarations restore conservative custody for uninspected
-- attempts; inspected recoveries and settled stops never regain their leases.
INSERT OR IGNORE INTO resource_leases(resource_id,dispatch_id,exclusive)
SELECT r.resource_id,r.dispatch_id,r.exclusive FROM dispatch_resources r
JOIN unresolved_dispatches u ON u.id=r.dispatch_id;
