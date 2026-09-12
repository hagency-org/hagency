CREATE TABLE role_publications (
    role TEXT PRIMARY KEY,
    published INTEGER NOT NULL CHECK(published IN (0,1))
) STRICT;
