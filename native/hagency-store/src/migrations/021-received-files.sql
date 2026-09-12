-- Private cache correlation facts; no descriptor, destination path or new authority.
CREATE TABLE received_files (
 id TEXT PRIMARY KEY,
 capability_digest TEXT NOT NULL,
 event_id TEXT NOT NULL,
 workspace_id TEXT NOT NULL REFERENCES workspace_resources(id),
 binding TEXT NOT NULL CHECK(json_valid(binding) AND length(binding)<=16384),
 binding_digest TEXT NOT NULL,
 byte_limit INTEGER NOT NULL CHECK(byte_limit BETWEEN 1 AND 4194304),
 facts TEXT CHECK(facts IS NULL OR json_valid(facts)),
 state TEXT NOT NULL CHECK(state IN ('reserved','write_possible','ready','failed','outcome_unknown')),
 failure TEXT CHECK(failure IS NULL OR failure IN ('cancelled','source_refused','write_refused','outcome_unknown')),
 UNIQUE(capability_digest,event_id),
 CHECK(state NOT IN ('write_possible','ready') OR facts IS NOT NULL),
 CHECK((state IN ('reserved','write_possible','ready') AND failure IS NULL) OR
       (state IN ('failed','outcome_unknown') AND failure IS NOT NULL))
);
CREATE INDEX received_workspace ON received_files(workspace_id);
