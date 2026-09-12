CREATE TABLE bindings (
    id TEXT PRIMARY KEY NOT NULL,
    generation INTEGER NOT NULL CHECK(generation > 0)
) STRICT;
CREATE TABLE inbox (
    binding TEXT NOT NULL REFERENCES bindings(id),
    lane TEXT NOT NULL CHECK(lane IN ('matrix','work')),
    id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation > 0),
    digest TEXT NOT NULL,
    payload TEXT NOT NULL,
    receipt TEXT NOT NULL,
    PRIMARY KEY(binding,lane,id)
) STRICT;
