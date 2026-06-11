CREATE TABLE workers (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL DEFAULT 'fixture-worker'
);

CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    worker_id INTEGER NOT NULL,
    FOREIGN KEY (worker_id) REFERENCES workers(id),
    CONSTRAINT chk_worker_id_positive CHECK (worker_id > 0)
);

CREATE VIEW active_workers AS
SELECT id, name
FROM workers
WHERE id > 0;

CREATE INDEX idx_workers_name ON workers (name);

WITH recent_workers AS (
    SELECT id, name FROM workers WHERE id > 0
)
SELECT w.id, w.name
FROM recent_workers rw
JOIN workers w ON rw.id = w.id;

BEGIN;
UPDATE workers SET name = 'updated' WHERE id = 1;
COMMIT;

CREATE TRIGGER refresh_active_workers
AFTER INSERT ON workers
FOR EACH ROW
BEGIN
    INSERT INTO jobs (worker_id)
    SELECT NEW.id
    FROM workers
    WHERE NEW.id > 0;
END;
