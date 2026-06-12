use anyhow::Result;
use rusqlite::params;

use crate::data::Db;
use crate::system::WorldTension;
use super::dialogue;
use super::local::{Facing, ItemKind, LocalMap, LOCAL_W};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel { Map, Log, Status, Codex }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Top-down overworld — moving between provinces
    Overworld,
    /// First-person inside a province
    FirstPerson,
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

/// What the interact menu is currently showing, if anything.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractMode {
    None,
    Menu,    // showing options
}


/// Cached province world state — loaded on enter_province, used for dialogue.
#[derive(Debug, Clone, Default)]
pub struct ProvinceContext {
    pub stability:    i64,
    pub famine:       i64,
    pub revolt_risk:  i64,
    pub faction_name: Option<String>,
    pub faction_tenet: Option<String>,
    pub at_war:       bool,
    pub war_attacker: Option<String>,
    pub war_defender: Option<String>,
    pub warlord_name: Option<String>,
    pub recent_event: Option<String>,
}

pub struct GameState {
    pub player:         Player,
    pub log:            Vec<LogEntry>,
    pub codex:          Vec<CodexEntry>,
    pub active_panel:   Panel,
    pub tension:        WorldTension,
    pub world_day:      i32,
    pub provinces_dark: u32,
    /// Set by the autopilot when drift mode is active — displayed in UI
    pub drifting:       bool,
    /// Cached world context for the current province — updated on entry
    pub province_ctx:   Option<ProvinceContext>,

    // ── First-person state ───────────────────────────────────────────────────
    pub view_mode:      ViewMode,
    /// Local map — Some when in first-person mode
    pub local_map:      Option<LocalMap>,
    /// Player's local position within the province map
    pub local_x:        i32,
    pub local_y:        i32,
    pub facing:         Facing,
    pub interact_mode:  InteractMode,
    /// Last interact message shown in the viewport
    pub viewport_msg:   Option<String>,
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
            drifting:       false,
            province_ctx:   None,
            view_mode:      ViewMode::Overworld,
            local_map:      None,
            local_x:        0,
            local_y:        0,
            facing:         Facing::North,
            interact_mode:  InteractMode::None,
            viewport_msg:   None,
        })
    }

    pub fn save(&self, db: &Db) -> Result<()> {
        db.conn.execute(
            "UPDATE player SET x=?1, y=?2, hp=?3, hunger=?4, fatigue=?5, province_id=?6
             WHERE id = 1",
            params![
                self.player.x, self.player.y, self.player.hp,
                self.player.hunger, self.player.fatigue, self.player.province_id,
            ],
        )?;
        db.set_meta("world_day", &self.world_day.to_string())?;
        Ok(())
    }

    pub fn push_log(&mut self, msg: impl Into<String>) {
        self.log.push(LogEntry { day: self.world_day, message: msg.into() });
        if self.log.len() > 220 { self.log.drain(0..20); }
    }

    pub fn push_codex(&mut self, day: i64, category: impl Into<String>, text: impl Into<String>) {
        self.codex.insert(0, CodexEntry { day, category: category.into(), text: text.into() });
        if self.codex.len() > 120 { self.codex.truncate(120); }
    }

    // ── Overworld movement ───────────────────────────────────────────────────

    pub fn move_player(&mut self, dx: i32, dy: i32) {
        self.player.x = self.player.x.saturating_add(dx).clamp(-128, 128);
        self.player.y = self.player.y.saturating_add(dy).clamp(-128, 128);
        self.player.fatigue = (self.player.fatigue + 1).min(100);
    }

    // ── Province entry / exit ────────────────────────────────────────────────

    pub fn enter_province(&mut self, db: &Db) -> Result<()> {
        let row: Option<(i64, String, String, i64, i64, i64)> = db.conn.query_row(
            "SELECT id, name, biome, stability, famine, revolt_risk
             FROM provinces WHERE x = ?1 AND y = ?2",
            params![self.player.x, self.player.y],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        ).ok();

        let Some((pid, pname, biome, stability, famine, revolt_risk)) = row else {
            self.push_log("Nothing here.");
            return Ok(());
        };

        // Load NPCs
        let npcs: Vec<(i64, String, String)> = {
            let mut stmt = db.conn.prepare(
                "SELECT id, name, role FROM npcs
                 WHERE province_id = ?1 AND alive = 1 LIMIT 5"
            )?;
            stmt.query_map(params![pid], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })
            .map(|rows| rows.filter_map(|r| r.ok()).collect::<Vec<_>>())
            .unwrap_or_default()
        };

        // Load controlling faction
        let faction: Option<(String, String)> = db.conn.query_row(
            "SELECT name, memory FROM factions
             WHERE province_id = ?1 AND status != 'collapsed'
             ORDER BY strength DESC LIMIT 1",
            params![pid],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ).ok();

        // Load faction tenet from lore
        let (faction_name, faction_tenet) = if let Some((ref fname, _)) = faction {
            let identity = crate::world::lore::faction_identity(fname);
            (Some(fname.clone()), Some(identity.tenet.to_string()))
        } else {
            (None, None)
        };

        // Active war?
        let war: Option<(String, String)> = db.conn.query_row(
            "SELECT fa.name, fd.name FROM wars w
             JOIN factions fa ON w.attacker_id = fa.id
             JOIN factions fd ON w.defender_id = fd.id
             WHERE w.province_id = ?1 LIMIT 1",
            params![pid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok();

        // Warlord present?
        let warlord: Option<String> = db.conn.query_row(
            "SELECT name FROM npcs
             WHERE province_id = ?1 AND role = 'warlord' AND alive = 1 LIMIT 1",
            params![pid],
            |r| r.get(0),
        ).ok();

        // Most recent codex event for this province
        let recent_event: Option<String> = db.conn.query_row(
            "SELECT entry FROM codex
             WHERE entry LIKE ?1
             ORDER BY id DESC LIMIT 1",
            params![format!("%{}%", pname)],
            |r| r.get(0),
        ).ok();

        let ctx = ProvinceContext {
            stability, famine, revolt_risk,
            faction_name: faction_name.clone(),
            faction_tenet: faction_tenet.clone(),
            at_war:       war.is_some(),
            war_attacker: war.as_ref().map(|(a, _)| a.clone()),
            war_defender: war.as_ref().map(|(_, d)| d.clone()),
            warlord_name: warlord.clone(),
            recent_event: recent_event.clone(),
        };

        let local = LocalMap::generate(pid, pname.clone(), biome, stability, npcs);

        self.local_x        = (LOCAL_W / 2) as i32;
        self.local_y        = (crate::world::local::LOCAL_H - 3) as i32;
        self.facing         = Facing::North;
        self.view_mode      = ViewMode::FirstPerson;
        self.local_map      = Some(local);
        self.province_ctx   = Some(ctx);
        self.player.province_id = pid;

        // Entry message reflects actual state
        let entry_note = if stability < 20 {
            format!("You enter {pname}. The destruction is extensive.")
        } else if famine > 65 {
            format!("You enter {pname}. The market stalls are empty.")
        } else if war.is_some() {
            format!("You enter {pname}. The sounds of fighting are not far.")
        } else if let Some(ref w) = warlord {
            format!("You enter {pname}. {w}'s soldiers are everywhere.")
        } else {
            format!("You enter {pname}.")
        };
        self.push_log(entry_note);
        Ok(())
    }

    pub fn exit_province(&mut self) {
        self.view_mode = ViewMode::Overworld;
        self.local_map = None;
        self.interact_mode = InteractMode::None;
        self.viewport_msg  = None;
        self.push_log("You return to the open road.");
    }

    // ── First-person movement ─────────────────────────────────────────────────

    /// Move forward one step in the current facing direction.
    pub fn fp_move_forward(&mut self) {
        let (ahead, npc_there) = {
            let Some(ref local) = self.local_map else { return; };
            let (dx, dy) = self.facing.delta();
            let nx = self.local_x + dx;
            let ny = self.local_y + dy;
            (local.cell(nx, ny), local.npc_at(nx, ny).is_some())
        };
        let (dx, dy) = self.facing.delta();
        let nx = self.local_x + dx;
        let ny = self.local_y + dy;

        if ahead == crate::world::local::Cell::Exit {
            self.exit_province();
            return;
        }
        if !ahead.passable() {
            self.viewport_msg = Some("The way is blocked.".into());
            return;
        }
        if npc_there {
            self.viewport_msg = Some("Something stands in your path. Use [.] to interact.".into());
            return;
        }

        self.local_x = nx;
        self.local_y = ny;
        self.player.fatigue = (self.player.fatigue + 1).min(100);

        // Surface ambient cell text on move — 25% of steps
        let ambient = self.local_map.as_ref().and_then(|local| {
            let seed = (nx as u32).wrapping_mul(1337).wrapping_add(ny as u32);
            dialogue::ambient_cell(&local.biome,
                self.province_ctx.as_ref().map(|c| c.stability).unwrap_or(60),
                self.world_day, seed)
        });
        self.viewport_msg = ambient.map(str::to_string);
    }

    pub fn fp_move_backward(&mut self) {
        let Some(ref local) = self.local_map else { return; };
        let (dx, dy) = self.facing.delta();
        let nx = self.local_x - dx;
        let ny = self.local_y - dy;
        if local.cell(nx, ny).passable() && local.npc_at(nx, ny).is_none() {
            self.local_x = nx;
            self.local_y = ny;
            self.viewport_msg = None;
        }
    }

    pub fn fp_strafe_left(&mut self) {
        let Some(ref local) = self.local_map else { return; };
        let (dx, dy) = self.facing.left().delta();
        let nx = self.local_x + dx;
        let ny = self.local_y + dy;
        if local.cell(nx, ny).passable() && local.npc_at(nx, ny).is_none() {
            self.local_x = nx;
            self.local_y = ny;
            self.viewport_msg = None;
        }
    }

    pub fn fp_strafe_right(&mut self) {
        let Some(ref local) = self.local_map else { return; };
        let (dx, dy) = self.facing.right().delta();
        let nx = self.local_x + dx;
        let ny = self.local_y + dy;
        if local.cell(nx, ny).passable() && local.npc_at(nx, ny).is_none() {
            self.local_x = nx;
            self.local_y = ny;
            self.viewport_msg = None;
        }
    }

    pub fn fp_turn_left(&mut self)  { self.facing = self.facing.left(); }
    pub fn fp_turn_right(&mut self) { self.facing = self.facing.right(); }

    // ── Interact / combat ─────────────────────────────────────────────────────

    pub fn fp_interact(&mut self, db: &mut Db) -> Result<()> {
        let (dx, dy) = self.facing.delta();
        let tx = self.local_x + dx;
        let ty = self.local_y + dy;

        // NPC in front?
        let npc_ahead = self.local_map.as_ref()
            .and_then(|m| m.npc_at(tx, ty))
            .map(|n| (n.name.clone(), n.role.clone(), n.hostile, n.hp, n.db_id));

        if let Some((name, role, hostile, hp, db_id)) = npc_ahead {
            if hostile {
                self.fp_attack(db, tx, ty, &name, hp, db_id)?;
            } else {
                // Build context-aware dialogue
                let ctx = self.make_world_context();
                let lines = dialogue::npc_lines(&name, &role, false, &ctx);
                let first = lines.first().cloned().unwrap_or_else(|| "...".into());
                let full  = lines.join(" / ");
                self.viewport_msg = Some(format!("{name}: {full}"));
                self.push_log(format!("{name}: \"{first}\""));
            }
            return Ok(());
        }

        // Item at feet?
        let item_idx = self.local_map.as_ref()
            .and_then(|m| m.item_at(self.local_x, self.local_y));
        if let Some(idx) = item_idx {
            if let Some(ref mut local) = self.local_map {
                let item = local.items.remove(idx);

                // Scrolls get province-specific text
                let effect = if matches!(item.kind, ItemKind::Scroll) {
                    let ctx_event = self.province_ctx.as_ref()
                        .and_then(|c| c.recent_event.as_deref());
                    let pname = self.local_map.as_ref()
                        .map(|l| l.province_name.as_str())
                        .unwrap_or("here");
                    dialogue::scroll_text(ctx_event, pname, self.world_day)
                } else {
                    apply_item(&item.kind, &mut self.player)
                };

                self.viewport_msg = Some(format!("{} — {effect}", item.name));
                self.push_log(format!("Picked up: {}.", item.name));
            }
            return Ok(());
        }

        // Door ahead?
        let ahead_cell = self.local_map.as_ref().map(|m| m.cell(tx, ty));
        if ahead_cell == Some(crate::world::local::Cell::Door) {
            self.viewport_msg = Some("You push through the door.".into());
            if let Some(ref mut local) = self.local_map {
                local.cells[ty as usize][tx as usize] = crate::world::local::Cell::Floor;
            }
            return Ok(());
        }

        self.viewport_msg = Some("Nothing here.".into());
        Ok(())
    }

    /// Build a WorldContext from cached province data for dialogue generation.
    fn make_world_context(&self) -> dialogue::WorldContext<'_> {
        let pname = self.local_map.as_ref()
            .map(|l| l.province_name.as_str())
            .unwrap_or("unknown");

        if let Some(ref ctx) = self.province_ctx {
            dialogue::WorldContext {
                day:           self.world_day,
                tension_label: self.tension.label(),
                province_name: pname,
                stability:     ctx.stability,
                famine:        ctx.famine,
                revolt_risk:   ctx.revolt_risk,
                faction_name:  ctx.faction_name.as_deref(),
                faction_tenet: ctx.faction_tenet.as_deref(),
                at_war:        ctx.at_war,
                war_attacker:  ctx.war_attacker.as_deref(),
                war_defender:  ctx.war_defender.as_deref(),
                warlord_name:  ctx.warlord_name.as_deref(),
                recent_event:  ctx.recent_event.as_deref(),
            }
        } else {
            dialogue::WorldContext::blank(self.world_day, pname)
        }
    }

    fn fp_attack(&mut self, db: &mut Db, tx: i32, ty: i32, name: &str, npc_hp: i32, db_id: i64) -> Result<()> {
        // Simple d6 combat — player always hits for now, enemy retaliates
        let player_dmg = 4 + (self.world_day % 3) as i32; // scales slightly with days survived
        let new_hp     = npc_hp - player_dmg;

        if new_hp <= 0 {
            // Kill it
            if let Some(ref mut local) = self.local_map {
                if let Some(npc) = local.npcs.iter_mut().find(|n| n.x == tx as usize && n.y == ty as usize) {
                    npc.alive = false;
                }
            }
            // Mark dead in DB if it's a real NPC
            if db_id > 0 {
                db.conn.execute("UPDATE npcs SET alive = 0 WHERE id = ?1", params![db_id])?;
            }
            self.viewport_msg = Some(format!("{name} falls."));
            self.push_log(format!("You killed {name}."));
        } else {
            // Update NPC hp in local map
            if let Some(ref mut local) = self.local_map {
                if let Some(npc) = local.npcs.iter_mut().find(|n| n.x == tx as usize && n.y == ty as usize) {
                    npc.hp = new_hp;
                }
            }
            // Enemy counterattack
            let enemy_dmg = 2 + (new_hp / 10);
            self.player.hp = (self.player.hp - enemy_dmg).max(0);
            self.viewport_msg = Some(format!(
                "You hit {name} for {player_dmg}. {name} strikes back for {enemy_dmg}."
            ));
            self.push_log(format!("Combat: {name} ({new_hp} hp left). You take {enemy_dmg} damage."));
        }
        Ok(())
    }

    // ── Panel controls ────────────────────────────────────────────────────────

    pub fn interact(&mut self, db: &mut Db) -> Result<()> {
        match self.view_mode {
            ViewMode::FirstPerson => self.fp_interact(db),
            ViewMode::Overworld   => {
                self.push_log("Press [Enter] to enter this province.");
                Ok(())
            }
        }
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
        self.active_panel = if self.active_panel == Panel::Codex { Panel::Map } else { Panel::Codex };
    }
}

// ── Pure helpers ──────────────────────────────────────────────────────────────

fn load_codex(db: &Db, limit: usize) -> Result<Vec<CodexEntry>> {
    Ok(db.recent_codex(limit)?.into_iter()
        .map(|(day, category, text)| CodexEntry { day, category, text })
        .collect())
}

fn apply_item(kind: &ItemKind, player: &mut Player) -> String {
    match kind {
        ItemKind::Food   => { player.hunger = (player.hunger + 25).min(100); "Hunger eased.".into() }
        ItemKind::Weapon => "You are better armed.".into(),
        ItemKind::Armor  => { player.max_hp = (player.max_hp + 5).min(150); "You feel more protected.".into() }
        ItemKind::Gold   => "The coin is yours.".into(),
        ItemKind::Scroll => "You read it.".into(),
    }
}

