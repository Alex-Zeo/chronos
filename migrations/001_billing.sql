-- 001_billing.sql
-- Chronos billing schema: 8 tables for automated time tracking.

CREATE TABLE client (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    contact     TEXT,
    rate_usd_hr REAL NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    notes       TEXT
);

CREATE TABLE billing_project (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    client_id       INTEGER NOT NULL REFERENCES client(id),
    name            TEXT NOT NULL,
    billing_type    TEXT NOT NULL CHECK(billing_type IN ('hourly', 'fixed', 'compute_only')),
    rate_override   REAL,
    budget_hours    REAL,
    status          TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'paused', 'closed')),
    created_at      TEXT NOT NULL,
    goals_json      TEXT,
    UNIQUE(client_id, name)
);

CREATE TABLE attribution_rule (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    billing_project_id  INTEGER NOT NULL REFERENCES billing_project(id),
    source              TEXT NOT NULL,
    rule_type           TEXT NOT NULL CHECK(rule_type IN ('channel', 'label', 'keyword', 'path', 'llm')),
    pattern             TEXT NOT NULL,
    priority            INTEGER NOT NULL DEFAULT 0,
    created_at          TEXT NOT NULL
);
CREATE INDEX idx_attr_rule_project ON attribution_rule(billing_project_id);
CREATE INDEX idx_attr_rule_source ON attribution_rule(source);

CREATE TABLE activity_event (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    source                  TEXT NOT NULL,
    source_event_id         TEXT NOT NULL,
    billing_project_id      INTEGER REFERENCES billing_project(id),
    event_type              TEXT NOT NULL,
    timestamp               TEXT NOT NULL,
    end_timestamp           TEXT,
    actor                   TEXT,
    summary                 TEXT,
    metadata_json           TEXT,
    preliminary_project_id  INTEGER,
    needs_llm_review        INTEGER NOT NULL DEFAULT 0,
    ingested_at             TEXT NOT NULL,
    UNIQUE(source, source_event_id)
);
CREATE INDEX idx_activity_event_project ON activity_event(billing_project_id);
CREATE INDEX idx_activity_event_ts ON activity_event(timestamp DESC);
CREATE INDEX idx_activity_event_source ON activity_event(source);
CREATE INDEX idx_activity_event_needs_review ON activity_event(needs_llm_review) WHERE needs_llm_review = 1;

CREATE TABLE time_block (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    billing_project_id  INTEGER NOT NULL REFERENCES billing_project(id),
    source              TEXT NOT NULL,
    start_ts            TEXT NOT NULL,
    end_ts              TEXT NOT NULL,
    duration_minutes    REAL NOT NULL,
    billing_rate        TEXT NOT NULL CHECK(billing_rate IN ('active', 'passive', 'compute_only', 'evidence')),
    rate_multiplier     REAL NOT NULL DEFAULT 1.0,
    cost_usd            REAL,
    parallel_index      INTEGER NOT NULL DEFAULT 0,
    parallel_total      INTEGER NOT NULL DEFAULT 1,
    parallel_label      TEXT,
    source_event_ids    TEXT,
    computed_at         TEXT NOT NULL
);
CREATE INDEX idx_time_block_project ON time_block(billing_project_id);
CREATE INDEX idx_time_block_ts ON time_block(start_ts DESC);
CREATE INDEX idx_time_block_parallel ON time_block(billing_project_id, parallel_index) WHERE parallel_index > 0;

CREATE TABLE decision_record (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    billing_project_id  INTEGER NOT NULL REFERENCES billing_project(id),
    summary             TEXT NOT NULL,
    alternatives        TEXT,
    rationale           TEXT,
    confidence          TEXT CHECK(confidence IN ('high', 'medium', 'low')),
    source              TEXT NOT NULL,
    source_event_id     TEXT,
    extracted_at        TEXT NOT NULL,
    content_hash        TEXT NOT NULL,
    consequence         TEXT,
    consequence_status  TEXT CHECK(consequence_status IN ('pending', 'confirmed', 'revised', 'abandoned'))
);
CREATE INDEX idx_decision_project ON decision_record(billing_project_id);
CREATE INDEX idx_decision_hash ON decision_record(content_hash);

CREATE TABLE work_item (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    billing_project_id  INTEGER NOT NULL REFERENCES billing_project(id),
    title               TEXT NOT NULL,
    description         TEXT,
    status              TEXT NOT NULL DEFAULT 'todo' CHECK(status IN ('todo', 'in_progress', 'done', 'blocked')),
    completion_pct      REAL NOT NULL DEFAULT 0,
    source              TEXT,
    source_ref          TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);
CREATE INDEX idx_work_item_project ON work_item(billing_project_id);
CREATE INDEX idx_work_item_status ON work_item(status);

CREATE TABLE connector_cursor (
    source      TEXT PRIMARY KEY,
    last_cursor TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
