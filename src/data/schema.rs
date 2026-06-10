use anyhow::Result;
use rusqlite::{Connection, params};
use std::io::Write;

pub struct Db {
    pub conn:  Connection,
    chronicle: std::fs::File,
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; \
             PRAGMA foreign_keys=ON; \
             PRAGMA synchronous=NORMAL;"
        )?;
        let chronicle = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("ironveil.chronicle")?;
        let db = Self { conn, chronicle };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(r"
            CREATE TABLE IF NOT EXISTS world_meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS provinces (
                id          INTEGER PRIMARY KEY,
                name        TEXT    NOT NULL,
                x           INTEGER NOT NULL,
                y           INTEGER NOT NULL,
                biome       TEXT    NOT NULL,
                stability   INTEGER NOT NULL DEFAULT 50,
                revolt_risk INTEGER NOT NULL DEFAULT 0,
                famine      INTEGER NOT NULL DEFAULT 0,
                known       INTEGER NOT NULL DEFAULT 0,
                dark        INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS factions (
                id             INTEGER PRIMARY KEY,
                name           TEXT    NOT NULL,
                archetype      TEXT    NOT NULL,
                province_id    INTEGER REFERENCES provinces(id),
                strength       INTEGER NOT NULL DEFAULT 50,
                morale         INTEGER NOT NULL DEFAULT 50,
                aggression     INTEGER NOT NULL DEFAULT 0,
                territory      INTEGER NOT NULL DEFAULT 1,
                status         TEXT    NOT NULL DEFAULT 'stable',
                container_name TEXT,
                memory         TEXT    NOT NULL DEFAULT '{}'
            );
            CREATE TABLE IF NOT EXISTS npcs (
                id          INTEGER PRIMARY KEY,
                name        TEXT    NOT NULL,
                faction_id  INTEGER REFERENCES factions(id),
                province_id INTEGER REFERENCES provinces(id),
                alive       INTEGER NOT NULL DEFAULT 1,
                role        TEXT    NOT NULL,
                reputation  INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS player (
                id          INTEGER PRIMARY KEY CHECK (id = 1),
                name        TEXT    NOT NULL,
                x           INTEGER NOT NULL DEFAULT 0,
                y           INTEGER NOT NULL DEFAULT 0,
                province_id INTEGER REFERENCES provinces(id),
                hp          INTEGER NOT NULL DEFAULT 100,
                max_hp      INTEGER NOT NULL DEFAULT 100,
                hunger      INTEGER NOT NULL DEFAULT 100,
                fatigue     INTEGER NOT NULL DEFAULT 0,
                generation  INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE IF NOT EXISTS skills (
                id        INTEGER PRIMARY KEY,
                player_id INTEGER REFERENCES player(id),
                name      TEXT    NOT NULL,
                level     INTEGER NOT NULL DEFAULT 1,
                last_used INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS reputation (
                player_id  INTEGER REFERENCES player(id),
                faction_id INTEGER REFERENCES factions(id),
                score      INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (player_id, faction_id)
            );
            CREATE TABLE IF NOT EXISTS rumors (
                id              INTEGER PRIMARY KEY,
                origin_province INTEGER REFERENCES provinces(id),
                target_province INTEGER REFERENCES provinces(id),
                content         TEXT    NOT NULL,
                accuracy        INTEGER NOT NULL DEFAULT 100,
                distance_hops   INTEGER NOT NULL DEFAULT 0,
                created_day     INTEGER NOT NULL,
                arrived_day     INTEGER
            );
            CREATE TABLE IF NOT EXISTS codex (
                id       INTEGER PRIMARY KEY,
                day      INTEGER NOT NULL,
                entry    TEXT    NOT NULL,
                category TEXT    NOT NULL DEFAULT 'event'
            );
            CREATE TABLE IF NOT EXISTS wars (
                id          INTEGER PRIMARY KEY,
                attacker_id INTEGER NOT NULL REFERENCES factions(id),
                defender_id INTEGER NOT NULL REFERENCES factions(id),
                started_day INTEGER NOT NULL,
                province_id INTEGER REFERENCES provinces(id),
                intensity   INTEGER NOT NULL DEFAULT 50
            );
            CREATE TABLE IF NOT EXISTS trade_routes (
                id            INTEGER PRIMARY KEY,
                province_a    INTEGER NOT NULL REFERENCES provinces(id),
                province_b    INTEGER NOT NULL REFERENCES provinces(id),
                disrupted     INTEGER NOT NULL DEFAULT 0,
                disrupted_day INTEGER
            );
            CREATE TABLE IF NOT EXISTS artifacts (
                id             INTEGER PRIMARY KEY,
                name           TEXT    NOT NULL,
                description    TEXT    NOT NULL,
                province_id    INTEGER REFERENCES provinces(id),
                held_by_npc    INTEGER REFERENCES npcs(id),
                held_by_player INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS wanted_notices (
                id          INTEGER PRIMARY KEY,
                faction_id  INTEGER REFERENCES factions(id),
                description TEXT    NOT NULL,
                bounty      INTEGER NOT NULL DEFAULT 0,
                created_day INTEGER NOT NULL
            );
        ")?;
        Ok(())
    }

    pub fn world_exists(&self) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM world_meta WHERE key = 'seed'",
            [],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO world_meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        match self.conn.query_row(
            "SELECT value FROM world_meta WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        ) {
            Ok(v)                                     => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e)                                    => Err(e.into()),
        }
    }

    pub fn write_codex(&mut self, day: i64, category: &str, entry: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO codex (day, category, entry) VALUES (?1, ?2, ?3)",
            params![day, category, entry],
        )?;
        writeln!(self.chronicle, "[Day {day}] ({category}) {entry}")?;
        Ok(())
    }

    pub fn recent_codex(&self, limit: usize) -> Result<Vec<(i64, String, String)>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut stmt = self.conn.prepare(
            "SELECT day, category, entry FROM codex ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        rows.map(|r| r.map_err(Into::into)).collect()
    }
}
