CREATE TABLE study_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    subject TEXT NOT NULL,
    duration_seconds INTEGER NOT NULL,
    date TEXT NOT NULL
);