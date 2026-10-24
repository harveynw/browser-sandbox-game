CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY);
CREATE TABLE profiles(
  profile_id TEXT PRIMARY KEY,
  cookie_secret_hash BLOB UNIQUE NOT NULL CHECK(length(cookie_secret_hash)=64),
  display_name TEXT NOT NULL,
  credits INTEGER NOT NULL CHECK(credits BETWEEN 0 AND 1000000000),
  completed_jobs INTEGER NOT NULL CHECK(completed_jobs BETWEEN 0 AND 4294967295),
  updated_at_unix_ms INTEGER NOT NULL
);
CREATE TABLE economic_events(
  event_id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL REFERENCES profiles(profile_id),
  kind TEXT NOT NULL CHECK(kind IN ('mission','repair','refund')),
  amount INTEGER NOT NULL,
  created_at_unix_ms INTEGER NOT NULL
);
CREATE INDEX economic_profile ON economic_events(profile_id);
CREATE TABLE repair_pending(
  event_id TEXT PRIMARY KEY REFERENCES economic_events(event_id),
  profile_id TEXT UNIQUE NOT NULL REFERENCES profiles(profile_id)
);
INSERT INTO schema_migrations VALUES(1);
PRAGMA user_version=1;
