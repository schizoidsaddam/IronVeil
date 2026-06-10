use anyhow::Result;
use rusqlite::params;

use crate::data::Db;
use crate::system::WorldTension;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Map,
    Log,
    Status,
    Codex,
}

#[derive(Debug, Clone)]
pub struct Player {
    pub name:        String,
    pub x:           i32,
    pub y:           i32,
    pub hp:          i32,
    pub max_hp:      i32,
    pub hunger:      i32,
    pub fatigue:     i32,
    pub generation:  i32,
    pub province_id: i64,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub day:     i32,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct CodexEntry {
    pub day:      i64,
    pub category: String,
    pub text:     String,
}

pub struct GameState {
    pub player:         Player,
    pub log:            Vec<LogEntry>,
    pub codex:          Vec<CodexEntry>,
    pub active_panel:   Panel,
    /// Current world tension — Copy type, stored directly (no String clone per tick)
    pub tension:        WorldTension,
    pub world_day:      i32,
    pub provinces_dark: u32,
}

impl GameState {
    pub fn load(db: &Db) -> Result<Self> {
        let player = db.conn.query_row(
            "SELECT name, x, y, hp, max_hp, hunger, fatigue, generation, province_id
             FROM player WHERE id = 1",
            [],
            |r| Ok(Player {
                name:        r.get(0)?,
                x:           r.get(1)?,
                y:           r.get(2)?,
                hp:          r.get(3)?,
                max_hp:      r.get(4)?,
                hunger:      r.get(5)?,
                fatigue:     r.get(6)?,
                generation:  r.get(7)?,
                province_id: r.get(8)?,
            }),
        )?;

        let world_day: i32 = db.get_meta("world_day")?
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);

        let provinces_dark: u32 = db.conn
            .query_row("SELECT COUNT(*) FROM provinces WHERE dark = 1", [], |r| r.get(0))
            .unwrap_or(0);

        let codex = load_codex(db, 60)?;

        Ok(Self {
            player,
            log:            Vec::with_capacity(220),
            codex,
            active_panel:   Panel::Map,
            tension:        WorldTension::Peaceful,
            world_day,
            provinces_dark,
        })
    }

    pub fn save(&self, db: &Db) -> Result<()> {
        db.conn.execute(
            "UPDATE player
             SET x=?1, y=?2, hp=?3, hunger=?4, fatigue=?5, province_id=?6
             WHERE id = 1",
            params![
                self.player.x,      self.player.y,
                self.player.hp,     self.player.hunger,
                self.player.fatigue, self.player.province_id,
            ],
        )?;
        db.set_meta("world_day", &self.world_day.to_string())?;
        Ok(())
    }

    pub fn push_log(&mut self, msg: impl Into<String>) {
        self.log.push(LogEntry { day: self.world_day, message: msg.into() });
        if self.log.len() > 220 {
            self.log.drain(0..20);
        }
    }

    /// Append a codex entry and prepend it to the in-memory list.
    pub fn push_codex(&mut self, day: i64, category: impl Into<String>, text: impl Into<String>) {
        self.codex.insert(0, CodexEntry {
            day,
            category: category.into(),
            text:     text.into(),
        });
        // Keep the last 120 in memory
        if self.codex.len() > 120 {
            self.codex.truncate(120);
        }
    }

    pub fn move_player(&mut self, dx: i32, dy: i32) {
        self.player.x = self.player.x.saturating_add(dx).clamp(-128, 128);
        self.player.y = self.player.y.saturating_add(dy).clamp(-128, 128);
        self.player.fatigue = (self.player.fatigue + 1).min(100);
    }

    pub fn interact(&mut self, _db: &Db) -> Result<()> {
        self.push_log("Nothing of note here.");
        Ok(())
    }

    pub fn cycle_panel(&mut self) {
        self.active_panel = match self.active_panel {
            Panel::Map    => Panel::Log,
            Panel::Log    => Panel::Status,
            Panel::Status => Panel::Codex,
            Panel::Codex  => Panel::Map,
        };
    }

    pub fn toggle_codex(&mut self) {
        self.active_panel = if self.active_panel == Panel::Codex {
            Panel::Map
        } else {
            Panel::Codex
        };
    }
}

fn load_codex(db: &Db, limit: usize) -> Result<Vec<CodexEntry>> {
    let entries = db.recent_codex(limit)?
        .into_iter()
        .map(|(day, category, text)| CodexEntry { day, category, text })
        .collect();
    Ok(entries)
}
