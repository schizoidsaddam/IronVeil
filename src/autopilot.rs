//! Drift mode autopilot with Director integration.
//!
//! Two modes of operation:
//!
//! 1. **Directed** — Director has built an itinerary of world events. Autopilot
//!    navigates to each stop in sequence, surfaces ambient dialogue from actual
//!    DB events, then moves to the next stop.
//!
//! 2. **Wandering** — No itinerary (world is calm, or all stops visited).
//!    Autopilot wanders the overworld and enters interesting-looking provinces.
//!
//! Any keypress immediately hands control back to the player.

use anyhow::Result;
use rand::{Rng, SeedableRng};
use rand::rngs::SmallRng;
use std::time::{Duration, Instant};

use crate::data::Db;
use crate::director::{self, DirectorStop};
use crate::world::GameState;
use crate::world::local::{Cell, Facing};
use crate::world::state::ViewMode;

pub const IDLE_THRESHOLD: u64 = 30;

/// How fast the autopilot steps when wandering (ms between moves)
const DRIFT_STEP_MS: u64 = 550;
/// How fast it steps when following the director's itinerary (slightly slower — cinematic)
const DIRECTED_STEP_MS: u64 = 750;
/// Steps to spend inside a province before moving to next stop
const PROVINCE_VISIT_STEPS: u32 = 45;
/// How many ambient lines to show per province visit
const AMBIENT_LINES_PER_STOP: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutopilotState {
    Idle,
    Drifting,
}

pub struct Autopilot {
    pub state:          AutopilotState,
    pub last_input:     Instant,
    last_step:          Instant,
    steps_in_province:  u32,
    rng:                SmallRng,
    blocked_streak:     u32,
    wander_dx:          i32,
    wander_dy:          i32,

    /// Director itinerary — None means wander freely
    itinerary:          Vec<DirectorStop>,
    /// Index into itinerary
    itinerary_pos:      usize,
    /// Ambient lines queued for current stop
    ambient_queue:      Vec<String>,
    /// Which ambient line to show next
    ambient_idx:        usize,
    /// Ticks until next ambient line surfaces
    ambient_countdown:  u32,
    /// Whether we've built this session's itinerary yet
    itinerary_built:    bool,
}

impl Autopilot {
    pub fn new() -> Self {
        Self {
            state:              AutopilotState::Idle,
            last_input:         Instant::now(),
            last_step:          Instant::now(),
            steps_in_province:  0,
            rng:                SmallRng::seed_from_u64(0xD41F_7123_4567_89AB),
            blocked_streak:     0,
            wander_dx:          0,
            wander_dy:          -1,
            itinerary:          Vec::new(),
            itinerary_pos:      0,
            ambient_queue:      Vec::new(),
            ambient_idx:        0,
            ambient_countdown:  0,
            itinerary_built:    false,
        }
    }

    pub fn on_input(&mut self) {
        self.last_input = Instant::now();
        self.state = AutopilotState::Idle;
    }

    pub fn tick(&mut self, game: &mut GameState, db: &mut Db) -> Result<()> {
        // Engage drift after idle threshold
        if self.state == AutopilotState::Idle
            && self.last_input.elapsed() >= Duration::from_secs(IDLE_THRESHOLD)
        {
            self.engage(game, db)?;
        }

        if self.state != AutopilotState::Drifting {
            return Ok(());
        }

        let step_ms = if self.is_directed() { DIRECTED_STEP_MS } else { DRIFT_STEP_MS };
        if self.last_step.elapsed() < Duration::from_millis(step_ms) {
            return Ok(());
        }
        self.last_step = Instant::now();

        // Tick ambient dialogue countdown
        if self.ambient_countdown > 0 {
            self.ambient_countdown -= 1;
        }
        if self.ambient_countdown == 0 && self.ambient_idx < self.ambient_queue.len() {
            let line = self.ambient_queue[self.ambient_idx].clone();
            game.viewport_msg = Some(line.clone());
            game.push_log(line);
            self.ambient_idx      += 1;
            self.ambient_countdown = 6; // ~4.5 seconds at 750ms/step before next line
        }

        match game.view_mode {
            ViewMode::Overworld   => self.drift_overworld(game, db)?,
            ViewMode::FirstPerson => self.drift_first_person(game, db)?,
        }

        Ok(())
    }

    fn engage(&mut self, game: &mut GameState, db: &mut Db) -> Result<()> {
        self.state = AutopilotState::Drifting;
        game.drifting = true;

        // Build itinerary once per drift session
        if !self.itinerary_built {
            self.itinerary = director::build_itinerary(db, game.world_day)?;
            self.itinerary_pos   = 0;
            self.itinerary_built = true;

            if self.itinerary.is_empty() {
                game.push_log("[ drifting — the world is quiet ]");
            } else {
                let first = &self.itinerary[0];
                game.push_log(format!(
                    "[ drifting — heading to {} ]",
                    first.province_name
                ));
            }
        } else {
            game.push_log("[ drifting ]");
        }

        Ok(())
    }

    fn is_directed(&self) -> bool {
        self.itinerary_pos < self.itinerary.len()
    }

    // ── Overworld navigation ──────────────────────────────────────────────────

    fn drift_overworld(&mut self, game: &mut GameState, db: &mut Db) -> Result<()> {
        if self.is_directed() {
            self.directed_overworld(game, db)
        } else {
            self.wander_overworld(game, db)
        }
    }

    fn directed_overworld(&mut self, game: &mut GameState, db: &mut Db) -> Result<()> {
        let stop = &self.itinerary[self.itinerary_pos];
        let tx   = stop.province_x;
        let ty   = stop.province_y;

        // Already at destination — enter it
        if game.player.x == tx && game.player.y == ty {
            // Load ambient for this stop
            self.ambient_queue    = stop.ambient.iter()
                .take(AMBIENT_LINES_PER_STOP)
                .cloned()
                .collect();
            self.ambient_idx      = 0;
            self.ambient_countdown = 4; // brief pause before first line

            game.enter_province(db)?;
            self.steps_in_province = 0;
            return Ok(());
        }

        // Step toward destination
        let dx = (tx - game.player.x).signum();
        let dy = (ty - game.player.y).signum();
        game.move_player(dx, dy);

        Ok(())
    }

    fn wander_overworld(&mut self, game: &mut GameState, db: &mut Db) -> Result<()> {
        // Seek interesting provinces opportunistically
        if self.rng.gen_bool(0.12) || self.province_is_interesting(game, db) {
            game.enter_province(db)?;
            self.steps_in_province = 0;
            self.ambient_queue     = Vec::new();
            return Ok(());
        }

        if self.rng.gen_bool(0.12) {
            self.pick_wander_direction();
        }

        let prev = (game.player.x, game.player.y);
        game.move_player(self.wander_dx, self.wander_dy);
        if (game.player.x, game.player.y) == prev {
            self.pick_wander_direction();
        }

        Ok(())
    }

    fn province_is_interesting(&self, game: &GameState, db: &Db) -> bool {
        db.conn.query_row(
            "SELECT COUNT(*) FROM provinces
             WHERE x = ?1 AND y = ?2
               AND (revolt_risk > 55 OR famine > 45 OR stability < 35)",
            rusqlite::params![game.player.x, game.player.y],
            |r| r.get::<_, i64>(0),
        ).unwrap_or(0) > 0
    }

    fn pick_wander_direction(&mut self) {
        let dirs: &[(i32, i32)] = &[(0,-1),(0,1),(-1,0),(1,0),(-1,-1),(1,-1),(-1,1),(1,1)];
        let (dx, dy) = dirs[self.rng.gen_range(0..dirs.len())];
        self.wander_dx = dx;
        self.wander_dy = dy;
    }

    // ── First-person navigation ───────────────────────────────────────────────

    fn drift_first_person(&mut self, game: &mut GameState, db: &mut Db) -> Result<()> {
        self.steps_in_province += 1;

        // Time to leave — either visited enough or out of ambient lines
        let ambient_exhausted = self.ambient_idx >= self.ambient_queue.len()
            && !self.ambient_queue.is_empty();
        let steps_done = self.steps_in_province >= PROVINCE_VISIT_STEPS;

        if steps_done || ambient_exhausted {
            self.advance_itinerary(game);
            return Ok(());
        }

        // Navigation
        let (ahead, npc_ahead) = {
            let Some(ref local) = game.local_map else {
                game.exit_province();
                return Ok(());
            };
            let (fdx, fdy) = game.facing.delta();
            let nx = game.local_x + fdx;
            let ny = game.local_y + fdy;
            (local.cell(nx, ny), local.npc_at(nx, ny).is_some())
        };

        if ahead == Cell::Exit {
            self.advance_itinerary(game);
            return Ok(());
        }

        if npc_ahead {
            // Observe, don't fight
            let obs = self.observe_npc(game);
            if let Some(msg) = obs {
                // Only surface observation if no ambient line pending
                if self.ambient_countdown == 0 || self.ambient_idx >= self.ambient_queue.len() {
                    game.viewport_msg = Some(msg);
                }
            }
            self.turn_to_unblock(game);
            self.blocked_streak = 0;
            return Ok(());
        }

        if !ahead.passable() {
            self.blocked_streak += 1;
            if self.blocked_streak >= 3 {
                self.turn_to_unblock(game);
                self.blocked_streak = 0;
            } else {
                self.turn_randomly(game);
            }
            return Ok(());
        }

        game.fp_move_forward();
        self.blocked_streak = 0;

        if self.rng.gen_bool(0.07) {
            self.turn_randomly(game);
        }

        let _ = db;
        Ok(())
    }

    fn advance_itinerary(&mut self, game: &mut GameState) {
        game.exit_province();
        self.steps_in_province = 0;
        self.ambient_queue     = Vec::new();
        self.ambient_idx       = 0;
        self.ambient_countdown = 0;

        if self.is_directed() {
            self.itinerary_pos += 1;
            if self.itinerary_pos < self.itinerary.len() {
                let next = &self.itinerary[self.itinerary_pos];
                game.push_log(format!("[ moving to {} ]", next.province_name));
            } else {
                game.push_log("[ itinerary complete — wandering ]");
            }
        }
    }

    fn observe_npc(&mut self, game: &GameState) -> Option<String> {
        let (fdx, fdy) = game.facing.delta();
        let tx = game.local_x + fdx;
        let ty = game.local_y + fdy;

        game.local_map.as_ref()?.npc_at(tx, ty).map(|npc| {
            drift_observation(&npc.name, &npc.role, npc.hostile)
        })
    }

    fn turn_randomly(&mut self, game: &mut GameState) {
        if self.rng.gen_bool(0.5) { game.fp_turn_left(); } else { game.fp_turn_right(); }
    }

    fn turn_to_unblock(&mut self, game: &mut GameState) {
        let best = {
            let Some(ref local) = game.local_map else { return; };
            let px = game.local_x;
            let py = game.local_y;
            let facings = [Facing::North, Facing::East, Facing::South, Facing::West];
            facings.iter()
                .filter(|&&f| f != game.facing)
                .max_by_key(|&&f| {
                    let (dx, dy) = f.delta();
                    (1..=3).filter(|&d| {
                        let c = local.cell(px + dx * d, py + dy * d);
                        c.passable() && c != Cell::Exit
                    }).count()
                })
                .copied()
        };

        if let Some(f) = best { game.facing = f; } else { game.fp_turn_right(); }
    }
}

fn drift_observation(name: &str, role: &str, hostile: bool) -> String {
    if hostile {
        let lines = [
            format!("{name} hasn't seen you yet."),
            format!("A {role}. Armed. You hold still."),
            format!("{name} paces the corridor."),
            format!("You watch {name} from the shadows."),
        ];
        lines[name.len() % lines.len()].clone()
    } else {
        let lines = [
            format!("{name} goes about their business."),
            format!("A {role}. They don't look up."),
            format!("{name} is here. Alive, for now."),
            format!("You observe {name} in the half-light."),
        ];
        lines[name.len() % lines.len()].clone()
    }
}
