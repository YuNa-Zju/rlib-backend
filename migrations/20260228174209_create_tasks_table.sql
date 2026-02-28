-- Add migration script here
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    password TEXT NOT NULL,
    target_date TEXT NOT NULL,
    area_id TEXT NOT NULL,
    interval_sec INTEGER NOT NULL
);
