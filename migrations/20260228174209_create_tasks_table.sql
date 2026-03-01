-- 确保任务表包含 area_name 和 start_time
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    password TEXT NOT NULL,
    target_date TEXT NOT NULL,
    area_id TEXT NOT NULL,
    area_name TEXT NOT NULL, -- 新增
    start_time TEXT NOT NULL, -- 新增
    interval_sec INTEGER NOT NULL
);
