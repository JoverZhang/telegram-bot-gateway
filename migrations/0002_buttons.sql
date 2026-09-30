CREATE TABLE message_keyboards(message INTEGER PRIMARY KEY REFERENCES messages(id),markup TEXT NOT NULL);
CREATE TABLE callbacks(id INTEGER PRIMARY KEY AUTOINCREMENT,query_id TEXT NOT NULL UNIQUE,agent TEXT NOT NULL REFERENCES agents(name),topic TEXT NOT NULL REFERENCES topics(id),message INTEGER NOT NULL REFERENCES messages(id),user INTEGER NOT NULL,data TEXT NOT NULL,received_at INTEGER NOT NULL);
CREATE INDEX callbacks_owner_order ON callbacks(agent,id);
CREATE INDEX callbacks_topic_order ON callbacks(agent,topic,id);
PRAGMA user_version=2;
