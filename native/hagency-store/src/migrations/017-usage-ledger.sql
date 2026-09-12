-- Host-attributed untrusted observations, never execution or quota authority.
CREATE TABLE usage_sources (
 id TEXT PRIMARY KEY, dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),
 fence INTEGER NOT NULL, engagement_id TEXT NOT NULL REFERENCES engagements(id),
 identity_digest TEXT NOT NULL, framework TEXT NOT NULL CHECK(framework IN ('claude','codex')),
 attribution TEXT NOT NULL CHECK(json_valid(attribution)),
 high_water TEXT NOT NULL CHECK(json_valid(high_water)),
 latest_counts TEXT NOT NULL CHECK(json_valid(latest_counts)),
 latest_observation TEXT CHECK(latest_observation IS NULL OR json_valid(latest_observation)),
 latest_incomplete INTEGER NOT NULL DEFAULT 1 CHECK(latest_incomplete IN (0,1)),
 latest_regressed INTEGER NOT NULL DEFAULT 0 CHECK(latest_regressed IN (0,1)),
 historical_incomplete INTEGER NOT NULL DEFAULT 0 CHECK(historical_incomplete IN (0,1)),
 regressions INTEGER NOT NULL DEFAULT 0, observations INTEGER NOT NULL DEFAULT 0,
 observed_at INTEGER, UNIQUE(dispatch_id,fence)
) STRICT;
CREATE INDEX usage_sources_engagement ON usage_sources(engagement_id);
CREATE TABLE usage_receipts (
 source_id TEXT NOT NULL REFERENCES usage_sources(id), call_id TEXT NOT NULL,
 digest TEXT NOT NULL, observation TEXT NOT NULL CHECK(json_valid(observation)),
 response TEXT NOT NULL CHECK(json_valid(response)), PRIMARY KEY(source_id,call_id)
) STRICT;
CREATE TABLE usage_periods (
 engagement_id TEXT NOT NULL REFERENCES engagements(id),
 granularity TEXT NOT NULL CHECK(granularity IN ('daily','monthly')), period_key TEXT NOT NULL,
 observed_growth TEXT NOT NULL CHECK(json_valid(observed_growth)),
 known_growth TEXT NOT NULL CHECK(json_valid(known_growth)),
 incomplete INTEGER NOT NULL CHECK(incomplete IN (0,1)), observations INTEGER NOT NULL,
 PRIMARY KEY(engagement_id,granularity,period_key)
) STRICT;
CREATE TABLE usage_clock (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1), observed_at INTEGER NOT NULL
) STRICT;
