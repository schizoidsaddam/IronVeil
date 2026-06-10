use anyhow::Result;
use rand::{Rng, SeedableRng};
use rand::rngs::SmallRng;
use rusqlite::params;
use sha2::{Digest, Sha256};

use crate::data::Db;
use crate::system::{SystemMetrics, WorldTension};
use crate::system::docker::{ContainerInfo, ContainerStatus};

use super::lore;
use super::names;
use super::state::GameState;

const THERMAL_LOG_RATE:   u64 = 10;
const ISOLATION_LOG_RATE: u64 = 20;
const RIFT_LOG_RATE:      u64 = 15;
const DAY_TICK_RATE:      u64 = 30;

pub struct WorldTick {
    tick_count:          u64,
    quake_cooldown:      u32,
    last_heat_log:       u64,
    last_isolation_log:  u64,
}

impl WorldTick {
    pub fn new() -> Self {
        Self { tick_count: 0, quake_cooldown: 0, last_heat_log: 0, last_isolation_log: 0 }
    }

    pub fn tick(
        &mut self,
        state:          &mut GameState,
        metrics:        &SystemMetrics,
        docker_metrics: &[ContainerInfo],
        db:             &mut Db,
    ) -> Result<()> {
        self.tick_count += 1;
        state.tension = metrics.tension;

        if self.tick_count % DAY_TICK_RATE == 0 {
            state.world_day += 1;
            db.set_meta("world_day", &state.world_day.to_string())?;
            let tick = self.tick_count;
            Self::age_world(state, metrics, db, tick)?;
            Self::propagate_rumors(state, db)?;
            Self::process_wars(state, db)?;
            Self::process_trade_routes(state, metrics, db)?;
            Self::process_time_of_day(state, metrics, db)?;
        }

        if self.tick_count % 5 == 0 && !docker_metrics.is_empty() {
            Self::process_factions(state, docker_metrics, db)?;
        }

        if metrics.disk.spike && self.quake_cooldown == 0 {
            Self::trigger_earthquake(state, db, self.tick_count)?;
            self.quake_cooldown = 60;
        }
        self.quake_cooldown = self.quake_cooldown.saturating_sub(1);

        if self.tick_count % ISOLATION_LOG_RATE == 0 {
            if metrics.network.silent {
                // Only log silence every 3 days (90 ticks) to avoid spam
                let silent_cooldown = 90u64;
                if self.tick_count.saturating_sub(self.last_isolation_log) >= silent_cooldown {
                    Self::process_isolation(state, db);
                    self.last_isolation_log = self.tick_count;
                } else {
                    // Still darken provinces silently, just don't log
                    db.conn.execute(
                        "UPDATE provinces SET famine = MIN(100, famine + 3) WHERE famine > 0",
                        [],
                    ).ok();
                }
            } else if state.provinces_dark > 0 {
                Self::process_network_return(state, db);
            }
        }

        if metrics.swap_pressure > 0.5 && self.tick_count % RIFT_LOG_RATE == 0 {
            Self::spawn_rift_event(state, metrics, db, self.tick_count)?;
        }

        self.process_thermals(state, metrics, db)?;

        Ok(())
    }

    // ── World aging ──────────────────────────────────────────────────────────

    fn age_world(
        state:   &mut GameState,
        metrics: &SystemMetrics,
        db:      &mut Db,
        tick:    u64,
    ) -> Result<()> {
        let mut rng = SmallRng::seed_from_u64(
            u64::from(state.world_day as u32) ^ 0xDEAD_C0DE,
        );
        let day     = i64::from(state.world_day);
        let variant = ((tick ^ (u64::from(state.world_day as u32).wrapping_mul(7))) & 0xFF) as u8;

        match metrics.tension {
            WorldTension::Peaceful => {
                db.conn.execute(
                    "UPDATE factions SET morale = MIN(100, morale + 1) WHERE status = 'stable'",
                    [],
                )?;
                // Stability slowly recovers during peace
                db.conn.execute(
                    "UPDATE provinces SET
                        stability   = MIN(100, stability + 1),
                        revolt_risk = MAX(0, revolt_risk - 2)
                     WHERE stability < 100",
                    [],
                )?;
            }
            WorldTension::Unrest => {
                if rng.gen_bool(0.3) {
                    let msg = lore::bandit_event(state.world_day, variant);
                    db.write_codex(day, "event", &msg)?;
                    state.push_codex(day, "event", &msg);
                    state.push_log(msg);
                    // Bandits disrupt trade routes
                    Self::disrupt_trade_routes(state, db, "bandit activity")?;
                    // Bandit activity raises revolt risk in unstable provinces
                    db.conn.execute(
                        "UPDATE provinces SET revolt_risk = MIN(100, revolt_risk + 5)
                         WHERE stability < 40",
                        [],
                    )?;
                }
            }
            WorldTension::Conflict => {
                if rng.gen_bool(0.4) {
                    Self::trigger_skirmish(state, db, &mut rng, variant)?;
                }
                // Conflict pressure builds revolt risk everywhere
                db.conn.execute(
                    "UPDATE provinces SET revolt_risk = MIN(100, revolt_risk + 3)",
                    [],
                )?;
            }
            WorldTension::Crisis => {
                if rng.gen_bool(0.15) {
                    Self::spawn_named_leader(state, db, tick)?;
                }
                if rng.gen_bool(0.2) {
                    let msg = lore::disaster_event(state.world_day, variant);
                    db.write_codex(day, "event", &msg)?;
                    state.push_codex(day, "event", &msg);
                    state.push_log(msg);
                    db.conn.execute(
                        "UPDATE factions SET morale = MAX(0, morale - 10) WHERE status = 'stable'",
                        [],
                    )?;
                    // Disasters spike revolt risk and famine
                    db.conn.execute(
                        "UPDATE provinces SET
                            revolt_risk = MIN(100, revolt_risk + 15),
                            famine      = MIN(100, famine + 10)",
                        [],
                    )?;
                }
                // Crisis: aggressive factions escalate much faster
                // Conquest/Restoration archetypes (order, compact, league) get extra aggression
                db.conn.execute(
                    "UPDATE factions SET
                        strength   = MIN(100, strength + 2),
                        aggression = MIN(100, aggression + 12)
                     WHERE status != 'collapsed'
                       AND (archetype = 'order' OR archetype = 'compact' OR archetype = 'league')",
                    [],
                )?;
                // All others still gain aggression, just slower
                db.conn.execute(
                    "UPDATE factions SET
                        aggression = MIN(100, aggression + 6)
                     WHERE status != 'collapsed'
                       AND archetype NOT IN ('order', 'compact', 'league')",
                    [],
                )?;
            }
        }

        // Check for revolts — provinces with revolt_risk >= 80
        Self::check_revolts(state, db, &mut rng, variant)?;

        // Aggression threshold: factions above 70 aggression declare war
        Self::check_war_declarations(state, db, &mut rng, variant)?;

        // Famine progression
        Self::process_famine(state, db, &mut rng, variant)?;

        // Player vitals
        state.player.hunger  = (state.player.hunger  - 1).max(0);
        state.player.fatigue = (state.player.fatigue - 2).max(0);

        if state.player.hunger == 0 {
            state.player.hp = (state.player.hp - 2).max(0);
            state.push_log("You are starving. Your body is failing.");
        }

        Ok(())
    }

    // ── Revolt system ────────────────────────────────────────────────────────

    fn check_revolts(
        state:   &mut GameState,
        db:      &mut Db,
        rng:     &mut SmallRng,
        variant: u8,
    ) -> Result<()> {
        // Find any province that has crossed the revolt threshold
        let revolt_candidates: Vec<(i64, String, i64, i64)> = {
            let mut stmt = db.conn.prepare(
                "SELECT id, name, revolt_risk, stability FROM provinces
                 WHERE revolt_risk >= 80
                 ORDER BY revolt_risk DESC
                 LIMIT 3"
            )?;
            stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
            }).map(|rows| rows.filter_map(|r: Result<_, _>| r.ok()).collect::<Vec<_>>()).unwrap_or_default()
        };

        for (pid, pname, risk, stability) in revolt_candidates {
            // Higher risk = higher revolt chance this day
            #[allow(clippy::cast_precision_loss)]
            let revolt_chance = (risk as f64 - 80.0) / 100.0;
            if !rng.gen_bool(revolt_chance) {
                continue;
            }

            let entry = lore::revolt_event(state.world_day, &pname, variant);
            let day   = i64::from(state.world_day);
            db.write_codex(day, "revolt", &entry)?;
            state.push_codex(day, "revolt", &entry);
            state.push_log(format!("REVOLT — {pname} has risen."));

            // Revolt destabilizes the province further
            db.conn.execute(
                "UPDATE provinces SET
                    stability   = MAX(0, stability - 30),
                    revolt_risk = 0
                 WHERE id = ?1",
                params![pid],
            )?;

            // Spawn a rumor about the revolt — it will spread outward
            Self::spawn_rumor(db, state.world_day, pid,
                &format!("{pname} is in open revolt. The streets are burning."))?;

            // If stability is critically low, the controlling faction collapses there
            if stability < 20 {
                Self::stability_collapse(state, db, pid, &pname, rng)?;
            }

            // Check if this revolt spreads to adjacent provinces
            // (shared biome = adjacent in our abstract map)
            db.conn.execute(
                "UPDATE provinces
                 SET revolt_risk = MIN(100, revolt_risk + 20)
                 WHERE biome = (SELECT biome FROM provinces WHERE id = ?1)
                   AND id != ?1",
                params![pid],
            )?;
        }

        Ok(())
    }

    fn stability_collapse(
        state:  &mut GameState,
        db:     &mut Db,
        pid:    i64,
        pname:  &str,
        rng:    &mut SmallRng,
    ) -> Result<()> {
        let entry = lore::stability_collapse(state.world_day, pname);
        let day   = i64::from(state.world_day);
        db.write_codex(day, "collapse", &entry)?;
        state.push_codex(day, "collapse", &entry);

        // The controlling faction loses morale and territory
        db.conn.execute(
            "UPDATE factions SET
                morale    = MAX(0, morale - 25),
                territory = MAX(1, territory - 1)
             WHERE province_id = ?1",
            params![pid],
        )?;

        // Power vacuum: an opportunistic Conquest or Restoration faction moves in
        let opportunist: Option<(i64, String)> = db.conn.query_row(
            "SELECT id, name FROM factions
             WHERE status = 'stable'
               AND province_id != ?1
               AND (archetype = 'compact' OR archetype = 'order' OR archetype = 'league')
             ORDER BY strength DESC
             LIMIT 1",
            params![pid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok();

        if let Some((fid, fname)) = opportunist {
            if rng.gen_bool(0.5) {
                let identity = lore::faction_identity(&fname);
                let msg      = lore::power_vacuum(state.world_day, pname, &fname);
                db.write_codex(day, "faction", &msg)?;
                state.push_codex(day, "faction", &msg);
                state.push_log(format!("{fname} claims the void in {pname}."));

                db.conn.execute(
                    "UPDATE factions SET
                        province_id = ?1,
                        territory   = territory + 1,
                        aggression  = MAX(0, aggression - 10)
                     WHERE id = ?2",
                    params![pid, fid],
                )?;

                // Spawn rumor about the takeover
                Self::spawn_rumor(db, state.world_day, pid,
                    &format!("{fname} has moved into {pname}. {} ", identity.tenet))?;
            }
        }

        Ok(())
    }

    // ── War system ───────────────────────────────────────────────────────────

    fn check_war_declarations(
        state:   &mut GameState,
        db:      &mut Db,
        rng:     &mut SmallRng,
        variant: u8,
    ) -> Result<()> {
        // Find high-aggression factions not already at war
        let aggressors: Vec<(i64, String, i64)> = {
            let mut stmt = db.conn.prepare(
                "SELECT f.id, f.name, f.aggression FROM factions f
                 WHERE f.aggression >= 55
                   AND f.status = 'stable'
                   AND NOT EXISTS (
                       SELECT 1 FROM wars w
                       WHERE w.attacker_id = f.id OR w.defender_id = f.id
                   )
                 ORDER BY f.aggression DESC
                 LIMIT 2"
            )?;
            stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            }).map(|rows| rows.filter_map(|r: Result<_, _>| r.ok()).collect::<Vec<_>>()).unwrap_or_default()
        };

        for (aid, aname, _) in aggressors {
            if !rng.gen_bool(0.6) { continue; }

            // Pick a defender — prefer factions with low morale
            let Some((did, dname, province_id)) = db.conn.query_row(
                "SELECT id, name, province_id FROM factions
                 WHERE id != ?1
                   AND status = 'stable'
                   AND NOT EXISTS (
                       SELECT 1 FROM wars w
                       WHERE w.attacker_id = id OR w.defender_id = id
                   )
                 ORDER BY morale ASC
                 LIMIT 1",
                params![aid],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)),
            ).ok() else { continue; };

            // Declare war
            db.conn.execute(
                "INSERT INTO wars (attacker_id, defender_id, started_day, province_id, intensity)
                 VALUES (?1, ?2, ?3, ?4, 50)",
                params![aid, did, state.world_day, province_id],
            )?;

            // Reset aggression on declaration
            db.conn.execute(
                "UPDATE factions SET aggression = 0 WHERE id = ?1",
                params![aid],
            )?;

            let attacker_id = lore::faction_identity(&aname);
            let entry       = lore::war_declaration(state.world_day, &aname, &dname, &attacker_id);
            let day         = i64::from(state.world_day);
            db.write_codex(day, "war", &entry)?;
            state.push_codex(day, "war", &entry);
            state.push_log(format!("WAR: {aname} → {dname}"));

            // Spawn rumor about the war
            Self::spawn_rumor(db, state.world_day, province_id,
                &format!("War has been declared between {aname} and {dname}."))?;

            // Trade routes between their provinces disrupted immediately
            Self::disrupt_trade_routes(state, db, "war")?;

            let _ = variant; // used by callsite for other events
        }

        Ok(())
    }

    fn process_wars(state: &mut GameState, db: &mut Db) -> Result<()> {
        let wars: Vec<(i64, i64, i64, i64, i64, i64)> = {
            let mut stmt = db.conn.prepare(
                "SELECT w.id, w.attacker_id, w.defender_id, w.province_id, w.intensity, w.started_day
                 FROM wars w"
            )?;
            stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?))
            }).map(|rows| rows.filter_map(|r: Result<_, _>| r.ok()).collect::<Vec<_>>()).unwrap_or_default()
        };

        let mut to_end: Vec<i64> = vec![];

        for (wid, aid, did, prov_id, intensity, started_day) in wars {
            let Some((aname, a_strength, a_morale)) = db.conn.query_row(
                "SELECT name, strength, morale FROM factions WHERE id = ?1",
                params![aid],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
            ).ok() else { to_end.push(wid); continue; };

            let Some((dname, d_strength, d_morale)) = db.conn.query_row(
                "SELECT name, strength, morale FROM factions WHERE id = ?1",
                params![did],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)),
            ).ok() else { to_end.push(wid); continue; };

            let day = i64::from(state.world_day);

            // War grinds both sides down each day
            let a_loss = (intensity / 10).max(1);
            let d_loss = (intensity / 10).max(1);
            db.conn.execute(
                "UPDATE factions SET
                    morale   = MAX(0, morale - ?1),
                    strength = MAX(0, strength - ?2)
                 WHERE id = ?3",
                params![a_loss, a_loss / 2, aid],
            )?;
            db.conn.execute(
                "UPDATE factions SET
                    morale   = MAX(0, morale - ?1),
                    strength = MAX(0, strength - ?2)
                 WHERE id = ?3",
                params![d_loss, d_loss / 2, did],
            )?;

            // War destabilizes the contested province
            db.conn.execute(
                "UPDATE provinces SET
                    stability   = MAX(0, stability - 5),
                    revolt_risk = MIN(100, revolt_risk + 8)
                 WHERE id = ?1",
                params![prov_id],
            )?;

            // Occasional escalation events
            let duration = state.world_day - started_day as i32;
            if duration % 5 == 0 {
                let prov_name: String = db.conn.query_row(
                    "SELECT name FROM provinces WHERE id = ?1",
                    params![prov_id],
                    |r| r.get(0),
                ).unwrap_or_else(|_| "the frontier".into());
                let entry = lore::war_escalation(state.world_day, &aname, &dname, &prov_name);
                db.write_codex(day, "war", &entry)?;
                state.push_codex(day, "war", &entry);
            }

            // End condition: either side collapses (strength or morale < 10)
            // Loser collapses, winner gains territory
            let attacker_broken = a_strength < 10 || a_morale < 10;
            let defender_broken = d_strength < 10 || d_morale < 10;

            if attacker_broken || defender_broken {
                let (winner_id, winner_name, loser_id, loser_name) = if defender_broken {
                    (aid, &aname, did, &dname)
                } else {
                    (did, &dname, aid, &aname)
                };

                let entry = format!(
                    "Day {}. The war between {} and {} ends. {} is broken. {} claims the field.",
                    state.world_day, aname, dname, loser_name, winner_name
                );
                db.write_codex(day, "war", &entry)?;
                state.push_codex(day, "war", &entry);
                state.push_log(format!("War ends: {winner_name} defeats {loser_name}."));

                db.conn.execute(
                    "UPDATE factions SET status = 'collapsed' WHERE id = ?1",
                    params![loser_id],
                )?;
                db.conn.execute(
                    "UPDATE factions SET
                        territory = territory + 1,
                        strength  = MIN(100, strength + 20)
                     WHERE id = ?1",
                    params![winner_id],
                )?;

                // Spawn rumor about war outcome
                Self::spawn_rumor(db, state.world_day, prov_id,
                    &format!("{winner_name} has crushed {loser_name}. The war is over."))?;

                to_end.push(wid);
            }
        }

        for wid in to_end {
            db.conn.execute("DELETE FROM wars WHERE id = ?1", params![wid])?;
        }

        Ok(())
    }

    // ── Rumor system ─────────────────────────────────────────────────────────

    fn spawn_rumor(db: &mut Db, day: i32, origin_province: i64, content: &str) -> Result<()> {
        // Target = random known province that isn't the origin
        let target: Option<i64> = db.conn.query_row(
            "SELECT id FROM provinces
             WHERE id != ?1 AND known = 1
             ORDER BY RANDOM() LIMIT 1",
            params![origin_province],
            |r| r.get(0),
        ).ok();

        let Some(target_id) = target else { return Ok(()); };

        db.conn.execute(
            "INSERT INTO rumors
             (origin_province, target_province, content, accuracy, distance_hops, created_day)
             VALUES (?1, ?2, ?3, 100, 0, ?4)",
            params![origin_province, target_id, content, day],
        )?;

        Ok(())
    }

    fn propagate_rumors(state: &mut GameState, db: &mut Db) -> Result<()> {
        // Each day, in-transit rumors advance one hop and degrade accuracy
        // Arrived rumors (arrived_day IS NOT NULL) get surfaced if they just arrived
        let day = i64::from(state.world_day);

        // Advance undelivered rumors
        db.conn.execute(
            "UPDATE rumors SET
                distance_hops = distance_hops + 1,
                accuracy      = MAX(20, accuracy - 10)
             WHERE arrived_day IS NULL",
            [],
        )?;

        // Deliver rumors that have traveled enough (3+ hops = 3 days in transit)
        // Network silence delays delivery
        let deliver_count: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM rumors
             WHERE arrived_day IS NULL AND distance_hops >= 3",
            [],
            |r| r.get(0),
        )?;

        if deliver_count > 0 {
            // Mark as arrived
            db.conn.execute(
                "UPDATE rumors SET arrived_day = ?1
                 WHERE arrived_day IS NULL AND distance_hops >= 3",
                params![day],
            )?;

            // Surface the arrived rumors as log/codex entries
            let arrived: Vec<(String, i64)> = {
                let mut stmt = db.conn.prepare(
                    "SELECT content, accuracy FROM rumors
                     WHERE arrived_day = ?1
                     ORDER BY accuracy DESC
                     LIMIT 3"
                )?;
                stmt.query_map(params![day], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                }).map(|rows| rows.filter_map(|r: Result<_, _>| r.ok()).collect::<Vec<_>>()).unwrap_or_default()
            };

            for (content, accuracy) in arrived {
                let entry = lore::rumor_event(state.world_day, &content, accuracy);
                db.write_codex(day, "rumor", &entry)?;
                state.push_codex(day, "rumor", &entry);

                // Low-accuracy rumors can THEMSELVES trigger revolt risk escalation
                // (misinformation spreads panic)
                if accuracy < 50 {
                    db.conn.execute(
                        "UPDATE provinces SET revolt_risk = MIN(100, revolt_risk + 10)
                         WHERE known = 1 AND ABS(RANDOM()) % 3 = 0",
                        [],
                    )?;
                }
            }
        }

        // Prune very old delivered rumors (> 30 days old)
        db.conn.execute(
            "DELETE FROM rumors WHERE arrived_day IS NOT NULL AND arrived_day < ?1 - 30",
            params![day],
        )?;

        Ok(())
    }

    // ── Famine system ────────────────────────────────────────────────────────

    fn process_famine(
        state:   &mut GameState,
        db:      &mut Db,
        rng:     &mut SmallRng,
        variant: u8,
    ) -> Result<()> {
        let famine_provinces: Vec<(i64, String, i64)> = {
            let mut stmt = db.conn.prepare(
                "SELECT id, name, famine FROM provinces WHERE famine > 60 LIMIT 3"
            )?;
            stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            }).map(|rows| rows.filter_map(|r: Result<_, _>| r.ok()).collect::<Vec<_>>()).unwrap_or_default()
        };

        for (pid, pname, famine_level) in famine_provinces {
            // Only log + spike revolt on severe/worsening famine (> 75), not every tick
            if famine_level > 75 && rng.gen_bool(0.3) {
                let entry = lore::famine_event(state.world_day, &pname, variant);
                let day   = i64::from(state.world_day);
                db.write_codex(day, "event", &entry)?;
                state.push_codex(day, "event", &entry);
                state.push_log(format!("Famine in {pname}."));

                // Revolt risk increase capped to avoid runaway feedback
                db.conn.execute(
                    "UPDATE provinces SET revolt_risk = MIN(100, revolt_risk + 10) WHERE id = ?1",
                    params![pid],
                )?;

                Self::spawn_rumor(db, state.world_day, pid,
                    &format!("{pname} is starving. The roads out are crowded with refugees."))?;
            }

            // Strong per-province decay — famine should resolve in ~10 days if not reinforced
            db.conn.execute(
                "UPDATE provinces SET famine = MAX(0, famine - 8) WHERE id = ?1",
                params![pid],
            )?;
        }

        // Global decay — always running, keeps total famine bounded
        db.conn.execute(
            "UPDATE provinces SET famine = MAX(0, famine - 3) WHERE famine > 0",
            [],
        )?;

        Ok(())
    }

    // ── Trade route system ───────────────────────────────────────────────────

    fn process_trade_routes(state: &mut GameState, metrics: &SystemMetrics, db: &mut Db) -> Result<()> {
        // Re-open disrupted trade routes over time (when tension eases)
        if metrics.tension == WorldTension::Peaceful {
            let restored: i64 = db.conn.query_row(
                "UPDATE trade_routes SET disrupted = 0, disrupted_day = NULL
                 WHERE disrupted = 1 AND ABS(RANDOM()) % 3 = 0
                 RETURNING COUNT(*)",
                [],
                |r| r.get(0),
            ).unwrap_or(0);

            if restored > 0 {
                // Restored trade routes reduce famine
                db.conn.execute(
                    "UPDATE provinces SET famine = MAX(0, famine - 3)",
                    [],
                )?;
            }
        }

        // Active disruptions increase famine in connected provinces
        let disrupted: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM trade_routes WHERE disrupted = 1",
            [],
            |r| r.get(0),
        )?;

        if disrupted > 0 {
            db.conn.execute(
                "UPDATE provinces SET famine = MIN(100, famine + 2)
                 WHERE id IN (
                     SELECT province_a FROM trade_routes WHERE disrupted = 1
                     UNION SELECT province_b FROM trade_routes WHERE disrupted = 1
                 )",
                [],
            )?;
        }

        let _ = state;
        Ok(())
    }

    fn disrupt_trade_routes(state: &mut GameState, db: &mut Db, cause: &str) -> Result<()> {
        // Disrupt a random active trade route
        let disrupted: Option<(i64, i64)> = db.conn.query_row(
            "SELECT province_a, province_b FROM trade_routes
             WHERE disrupted = 0
             ORDER BY RANDOM() LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok();

        let Some((pa, pb)) = disrupted else { return Ok(()); };

        db.conn.execute(
            "UPDATE trade_routes SET disrupted = 1, disrupted_day = ?1
             WHERE province_a = ?2 AND province_b = ?3",
            params![state.world_day, pa, pb],
        )?;

        let pa_name: String = db.conn.query_row(
            "SELECT name FROM provinces WHERE id = ?1", params![pa], |r| r.get(0),
        ).unwrap_or_else(|_| "a province".into());
        let pb_name: String = db.conn.query_row(
            "SELECT name FROM provinces WHERE id = ?1", params![pb], |r| r.get(0),
        ).unwrap_or_else(|_| "another province".into());

        let entry = lore::trade_disruption(state.world_day, &pa_name, &pb_name, cause);
        let day   = i64::from(state.world_day);
        db.write_codex(day, "event", &entry)?;
        state.push_codex(day, "event", &entry);

        Ok(())
    }

    // ── Docker → Faction sync ────────────────────────────────────────────────

    fn process_factions(
        state:          &mut GameState,
        docker_metrics: &[ContainerInfo],
        db:             &mut Db,
    ) -> Result<()> {
        for container in docker_metrics {
            let existing: Option<(i64, String)> = db.conn.query_row(
                "SELECT id, status FROM factions WHERE container_name = ?1",
                params![&container.name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ).ok();

            let new_status = container_to_faction_status(&container.status);

            match existing {
                Some((fid, ref old_status)) if old_status != new_status => {
                    let faction_name: String = db.conn.query_row(
                        "SELECT name FROM factions WHERE id = ?1",
                        params![fid],
                        |r| r.get(0),
                    )?;
                    let identity = lore::faction_identity_from_container(&container.name);
                    let entry    = faction_transition_entry(&faction_name, new_status, &identity);
                    let day      = i64::from(state.world_day);
                    db.write_codex(day, "faction", &entry)?;
                    state.push_codex(day, "faction", &entry);
                    state.push_log(entry);
                    db.conn.execute(
                        "UPDATE factions SET status = ?1 WHERE id = ?2",
                        params![new_status, fid],
                    )?;

                    // Faction collapse destabilizes its province
                    if new_status == "collapsed" {
                        db.conn.execute(
                            "UPDATE provinces SET
                                stability   = MAX(0, stability - 20),
                                revolt_risk = MIN(100, revolt_risk + 25)
                             WHERE id = (SELECT province_id FROM factions WHERE id = ?1)",
                            params![fid],
                        )?;
                    }
                }
                Some(_) => {
                    db.conn.execute(
                        "UPDATE factions SET status = ?1 WHERE container_name = ?2",
                        params![new_status, &container.name],
                    )?;
                }
                None => {
                    if container.status == ContainerStatus::Running {
                        Self::spawn_container_faction(state, db, container)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn spawn_container_faction(state: &mut GameState, db: &mut Db, container: &ContainerInfo) -> Result<()> {
        let faction_name = names::faction_name_from_container(&container.name);
        let archetype    = names::faction_archetype_from_container(&container.name);
        let identity     = lore::faction_identity_from_container(&container.name);

        let province_id: i64 = db.conn.query_row(
            "SELECT id FROM provinces ORDER BY RANDOM() LIMIT 1",
            [],
            |r| r.get(0),
        )?;

        db.conn.execute(
            "INSERT INTO factions
             (name, archetype, province_id, strength, morale, aggression, territory, status, container_name)
             VALUES (?1, ?2, ?3, 50, 60, 0, 1, 'stable', ?4)",
            params![faction_name, archetype, province_id, &container.name],
        )?;

        let entry = format!(
            "A new {archetype} rises — {faction_name}. {goal} {declaration} Their tenet: \"{tenet}\"",
            goal        = goal_preamble(identity.goal),
            declaration = identity.founding_declaration,
            tenet       = identity.tenet,
        );
        let day = i64::from(state.world_day);
        db.write_codex(day, "faction", &entry)?;
        state.push_codex(day, "faction", &entry);
        state.push_log(format!("{faction_name} — {}", identity.tenet));
        Ok(())
    }

    // ── Environmental events ─────────────────────────────────────────────────

    fn trigger_earthquake(state: &mut GameState, db: &mut Db, tick: u64) -> Result<()> {
        let variant = ((tick ^ (u64::from(state.world_day as u32).wrapping_mul(7))) & 0xFF) as u8;
        let entry   = lore::earthquake_event(state.world_day, variant);
        let day     = i64::from(state.world_day);
        db.write_codex(day, "event", &entry)?;
        state.push_codex(day, "event", &entry);
        state.push_log("The earth moves. Something below has shifted.");

        // Destabilize a province and spike its revolt risk + famine
        db.conn.execute(
            "UPDATE provinces SET
                stability   = MAX(0, stability - 20),
                revolt_risk = MIN(100, revolt_risk + 15),
                famine      = MIN(100, famine + 10)
             WHERE id = (SELECT id FROM provinces ORDER BY RANDOM() LIMIT 1)",
            [],
        )?;

        // Earthquake disrupts trade
        Self::disrupt_trade_routes(state, db, "earthquake damage")?;

        // Spawn rumor about the earthquake
        let prov_id: i64 = db.conn.query_row(
            "SELECT id FROM provinces ORDER BY revolt_risk DESC LIMIT 1",
            [],
            |r| r.get(0),
        ).unwrap_or(1);
        Self::spawn_rumor(db, state.world_day, prov_id,
            "The earth shook. Whole districts have gone silent.")?;

        Ok(())
    }

    fn process_isolation(state: &mut GameState, db: &mut Db) {
        // Network silence stops rumor propagation — freeze rumors in transit
        // Also: isolated provinces can't receive aid, famine worsens faster
        db.conn.execute(
            "UPDATE provinces SET famine = MIN(100, famine + 3) WHERE famine > 0",
            [],
        ).ok();

        let darkened: i64 = db.conn.query_row(
            "UPDATE provinces
             SET dark = 1
             WHERE known = 1 AND dark = 0
               AND id != (SELECT province_id FROM player WHERE id = 1)
               AND ABS(RANDOM()) % 3 = 0
             RETURNING COUNT(*)",
            [],
            |r| r.get(0),
        ).unwrap_or(0);

        if darkened > 0 {
            let n = u32::try_from(darkened).unwrap_or(u32::MAX);
            state.provinces_dark = state.provinces_dark.saturating_add(n);
            state.push_log(format!("Silence falls. {darkened} province(s) go dark. Rumors freeze."));
        } else if state.provinces_dark == 0 {
            state.push_log("The roads have gone quiet. No word from distant provinces.");
        }
    }

    fn process_network_return(state: &mut GameState, db: &mut Db) {
        let restored: i64 = db.conn.query_row(
            "UPDATE provinces SET dark = 0
             WHERE dark = 1 AND ABS(RANDOM()) % 4 = 0
             RETURNING COUNT(*)",
            [],
            |r| r.get(0),
        ).unwrap_or(0);

        if restored > 0 {
            let n = u32::try_from(restored).unwrap_or(u32::MAX);
            state.provinces_dark = state.provinces_dark.saturating_sub(n);
            state.push_log(format!("The roads are open. Word floods in from {restored} province(s)."));
        }
    }

    fn spawn_rift_event(state: &mut GameState, metrics: &SystemMetrics, db: &mut Db, tick: u64) -> Result<()> {
        let variant = ((tick ^ (u64::from(state.world_day as u32).wrapping_mul(7))) & 0xFF) as u8;
        let entry   = lore::rift_event(state.world_day, metrics.swap_pressure, variant);
        let day     = i64::from(state.world_day);
        db.write_codex(day, "omen", &entry)?;
        state.push_codex(day, "omen", &entry);
        state.push_log("Something is wrong with the geometry of things.");

        // Rift events tank morale on factions near the affected province
        db.conn.execute(
            "UPDATE factions SET morale = MAX(0, morale - 8)
             WHERE status = 'stable' AND ABS(RANDOM()) % 2 = 0",
            [],
        )?;

        // And raise revolt risk — people are scared
        db.conn.execute(
            "UPDATE provinces SET revolt_risk = MIN(100, revolt_risk + 12)
             WHERE ABS(RANDOM()) % 3 = 0",
            [],
        )?;

        Ok(())
    }

    fn process_thermals(&mut self, state: &mut GameState, metrics: &SystemMetrics, db: &mut Db) -> Result<()> {
        let max_temp    = metrics.thermals.iter().map(|t| t.temp_c).fold(0.0_f32, f32::max);
        let ticks_since = self.tick_count.saturating_sub(self.last_heat_log);

        if max_temp >= 90.0 && ticks_since >= THERMAL_LOG_RATE {
            state.push_log(format!(
                "A killing heat — {max_temp:.0}°C. The fields are ash. Water is scarce."
            ));
            self.last_heat_log = self.tick_count;
            // Severe heat drives famine everywhere
            db.conn.execute(
                "UPDATE provinces SET famine = MIN(100, famine + 15)",
                [],
            )?;
        } else if (80.0..90.0).contains(&max_temp) && ticks_since >= THERMAL_LOG_RATE * 3 {
            state.push_log("The heat is relentless. The harvest will be short.");
            self.last_heat_log = self.tick_count;
            db.conn.execute(
                "UPDATE provinces SET famine = MIN(100, famine + 5)",
                [],
            )?;
        }

        Ok(())
    }

    fn process_time_of_day(state: &mut GameState, metrics: &SystemMetrics, db: &mut Db) -> Result<()> {
        match metrics.hour_of_day {
            0..=3 if state.world_day % 7 == 0 => {
                let omens: &[&str] = &[
                    "Something walks the old roads that does not sleep and does not stop.",
                    "The dogs refused to make sound between midnight and the fourth hour.",
                    "Three sentries reported the same figure at the edge of the firelight. None woke the others.",
                    "The candles in the lower districts burned blue for an hour and then resumed.",
                ];
                let idx   = (state.world_day as u8) as usize % omens.len();
                let entry = format!("Day {}. {}", state.world_day, omens[idx]);
                let day   = i64::from(state.world_day);
                db.write_codex(day, "omen", &entry)?;
                state.push_codex(day, "omen", &entry);
            }
            6..=8 => {
                let restored: i64 = db.conn.query_row(
                    "UPDATE provinces SET dark = 0
                     WHERE dark = 1 AND ABS(RANDOM()) % 3 = 0
                     RETURNING COUNT(*)",
                    [],
                    |r| r.get(0),
                ).unwrap_or(0);
                if restored > 0 {
                    let n = u32::try_from(restored).unwrap_or(u32::MAX);
                    state.provinces_dark = state.provinces_dark.saturating_sub(n);
                }
            }
            _ => {}
        }
        Ok(())
    }

    // ── Named events ─────────────────────────────────────────────────────────

    fn trigger_skirmish(
        state:   &mut GameState,
        db:      &mut Db,
        rng:     &mut SmallRng,
        variant: u8,
    ) -> Result<()> {
        let Some(f1) = db.conn.query_row(
            "SELECT id, name FROM factions WHERE status = 'stable' ORDER BY RANDOM() LIMIT 1",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        ).ok() else { return Ok(()); };

        let Some(f2) = db.conn.query_row(
            "SELECT id, name FROM factions WHERE status = 'stable' AND id != ?1
             ORDER BY RANDOM() LIMIT 1",
            params![f1.0],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        ).ok() else { return Ok(()); };

        let winner_idx  = usize::from(rng.gen_bool(0.5));
        let loser_idx   = 1 - winner_idx;
        let factions    = [(&f1.1, f1.0), (&f2.1, f2.0)];
        let winner_name = factions[winner_idx].0.as_str();
        let loser_name  = factions[loser_idx].0.as_str();
        let loser_id    = factions[loser_idx].1;

        let winner_identity = lore::faction_identity(winner_name);
        let loser_identity  = lore::faction_identity(loser_name);

        let entry = lore::skirmish_entry(
            state.world_day, winner_name, loser_name,
            &winner_identity, &loser_identity, variant,
        );
        let day = i64::from(state.world_day);
        db.write_codex(day, "conflict", &entry)?;
        state.push_codex(day, "conflict", &entry);
        state.push_log(format!("Skirmish: {winner_name} prevails over {loser_name}."));

        // Loser: morale hit + aggression builds (they want revenge)
        db.conn.execute(
            "UPDATE factions SET
                morale     = MAX(0, morale - 15),
                aggression = MIN(100, aggression + 20)
             WHERE id = ?1",
            params![loser_id],
        )?;

        // Skirmish destabilizes the contested territory slightly
        db.conn.execute(
            "UPDATE provinces SET stability = MAX(0, stability - 8)
             WHERE id IN (
                 SELECT province_id FROM factions
                 WHERE id = ?1 OR id = ?2
             )",
            params![f1.0, f2.0],
        )?;

        // Spawn rumor about the outcome
        let prov_id: i64 = db.conn.query_row(
            "SELECT province_id FROM factions WHERE id = ?1",
            params![loser_id],
            |r| r.get(0),
        ).unwrap_or(1);
        Self::spawn_rumor(db, state.world_day, prov_id,
            &format!("{winner_name} defeated {loser_name} in a border engagement."))?;

        Ok(())
    }

    fn spawn_named_leader(state: &mut GameState, db: &mut Db, tick: u64) -> Result<()> {
        let seed_input  = format!("leader:{}:{tick}", state.world_day);
        let hash        = Sha256::digest(seed_input.as_bytes());
        let name_seed   = u64::from_le_bytes(hash[..8].try_into().unwrap());
        let leader_name = names::npc_name(name_seed);

        let Some((faction_id, province_id, faction_name)) = db.conn.query_row(
            "SELECT id, province_id, name FROM factions
             WHERE status != 'collapsed'
             ORDER BY RANDOM() LIMIT 1",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)),
        ).ok() else { return Ok(()); };

        db.conn.execute(
            "INSERT INTO npcs (name, faction_id, province_id, alive, role, reputation)
             VALUES (?1, ?2, ?3, 1, 'warlord', 50)",
            params![leader_name, faction_id, province_id],
        )?;

        let identity = lore::faction_identity(&faction_name);
        let entry    = lore::warlord_entry(state.world_day, &leader_name, &faction_name, &identity);
        let day      = i64::from(state.world_day);
        db.write_codex(day, "named_entity", &entry)?;
        state.push_codex(day, "named_entity", &entry);
        state.push_log(format!("{leader_name} rises from {faction_name}."));

        // Warlords immediately boost their faction's aggression
        db.conn.execute(
            "UPDATE factions SET aggression = MIN(100, aggression + 30) WHERE id = ?1",
            params![faction_id],
        )?;

        // And spawn a rumor that spreads fear
        Self::spawn_rumor(db, state.world_day, province_id,
            &format!("{leader_name} commands {faction_name}. Their armies are moving."))?;

        Ok(())
    }
}

// ── Pure helpers ─────────────────────────────────────────────────────────────

fn container_to_faction_status(status: &ContainerStatus) -> &'static str {
    match status {
        ContainerStatus::Running    => "stable",
        ContainerStatus::Stopped    => "collapsed",
        ContainerStatus::Starting   => "interregnum",
        ContainerStatus::Restarting => "civil_war",
        ContainerStatus::Other(_)   => "unstable",
    }
}

fn faction_transition_entry(name: &str, new_status: &str, identity: &lore::FactionIdentity) -> String {
    match new_status {
        "collapsed"   => format!("{name} is gone. {}", identity.collapse_voice),
        "civil_war"   => format!("{name} — {}", identity.civil_war_voice),
        "interregnum" => format!("{name}: {}", identity.interregnum_voice),
        "stable"      => format!("{name}. {}", identity.stable_voice),
        _             => format!("{name} is in an uncertain state."),
    }
}

fn goal_preamble(goal: lore::FactionGoal) -> &'static str {
    match goal {
        lore::FactionGoal::Commerce     => "Merchants and factors, organized for profit.",
        lore::FactionGoal::Conquest     => "An armed body with territorial ambition.",
        lore::FactionGoal::Preservation => "Scholars and keepers, obsessed with what must not be lost.",
        lore::FactionGoal::Purity       => "True believers, organized around the removal of corruption.",
        lore::FactionGoal::Survival     => "A people who have decided survival is not optional.",
        lore::FactionGoal::Restoration  => "Those who remember what was, and intend to restore it.",
        lore::FactionGoal::Dominion     => "Watchers and information brokers, patient and connected.",
        lore::FactionGoal::Ascension    => "Seekers of something beyond the material world.",
    }
}
