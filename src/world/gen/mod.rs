#![allow(clippy::cast_possible_wrap)]  // WORLD_W/H are small constants, wrap impossible

use anyhow::Result;
use rand::{Rng, SeedableRng};
use rand::rngs::SmallRng;
use rusqlite::params;

use crate::data::Db;
use super::names;

const WORLD_W: usize = 16;
const WORLD_H: usize = 12;

/// Generate a fresh world seeded from the machine fingerprint.
/// All inserts are inside one transaction — atomic or nothing.
pub fn generate_world(db: &mut Db, seed: u64) -> Result<()> {
    let mut rng = SmallRng::seed_from_u64(seed);

    db.conn.execute_batch("BEGIN;")?;

    let result = populate(db, seed, &mut rng);

    match result {
        Ok(()) => {
            db.conn.execute_batch("COMMIT;")?;
            db.write_codex(
                1,
                "genesis",
                "In the first day of the Ironveil, the world was seeded from the bones \
                 of the machine. What follows is its chronicle.",
            )?;
            Ok(())
        }
        Err(e) => {
            let _ = db.conn.execute_batch("ROLLBACK;");
            Err(e)
        }
    }
}

fn populate(db: &mut Db, seed: u64, rng: &mut SmallRng) -> Result<()> {
    db.set_meta("seed",      &seed.to_string())?;
    db.set_meta("world_w",   &WORLD_W.to_string())?;
    db.set_meta("world_h",   &WORLD_H.to_string())?;
    db.set_meta("world_day", "1")?;

    seed_provinces(db, seed, rng)?;
    seed_factions(db, seed, rng)?;
    seed_player(db)?;
    seed_npcs(db, seed, rng)?;
    seed_trade_routes(db, rng)?;

    Ok(())
}

fn seed_provinces(db: &mut Db, seed: u64, rng: &mut SmallRng) -> Result<()> {
    let start_x = (WORLD_W / 2) as i64;
    let start_y = (WORLD_H / 2) as i64;

    for y in 0..WORLD_H {
        for x in 0..WORLD_W {
            let province_seed = seed.wrapping_add((y * WORLD_W + x) as u64);
            let name  = names::province_name(province_seed);
            let biome = random_choice(rng, BIOMES);
            let known = i64::from(x as i64 == start_x && y as i64 == start_y);

            db.conn.execute(
                "INSERT INTO provinces (name, x, y, biome, stability, known, dark)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
                params![name, x as i64, y as i64, biome, rng.gen_range(40..80i64), known],
            )?;
        }
    }
    Ok(())
}

fn seed_factions(db: &mut Db, seed: u64, rng: &mut SmallRng) -> Result<()> {
    let num_factions = (WORLD_W * WORLD_H) / 4;

    for i in 0..num_factions {
        let faction_seed = seed.wrapping_add(0xFACE_0000 + i as u64);
        let name      = names::province_name(faction_seed);
        let archetype = random_choice(rng, ARCHETYPES);

        let province_id: i64 = db.conn.query_row(
            "SELECT id FROM provinces ORDER BY RANDOM() LIMIT 1",
            [],
            |r| r.get(0),
        )?;

        let aggression: i64 = match archetype {
            "order" | "compact" | "league" => rng.gen_range(30..55i64),
            "covenant" | "brotherhood"     => rng.gen_range(15..35i64),
            _                              => rng.gen_range(0..20i64),
        };

        db.conn.execute(
            "INSERT INTO factions (name, archetype, province_id, strength, morale, aggression, territory, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 'stable')",
            params![name, archetype, province_id, rng.gen_range(30..80i64), rng.gen_range(40..90i64), aggression],
        )?;
    }
    Ok(())
}

fn seed_player(db: &mut Db) -> Result<()> {
    let start_x = (WORLD_W / 2) as i64;
    let start_y = (WORLD_H / 2) as i64;

    let start_province: i64 = db.conn.query_row(
        "SELECT id FROM provinces WHERE x = ?1 AND y = ?2",
        params![start_x, start_y],
        |r| r.get(0),
    )?;

    db.conn.execute(
        "INSERT OR REPLACE INTO player
         (id, name, x, y, province_id, hp, max_hp, hunger, fatigue, generation)
         VALUES (1, 'Wanderer', ?1, ?2, ?3, 100, 100, 100, 0, 1)",
        params![start_x, start_y, start_province],
    )?;
    Ok(())
}

fn seed_npcs(db: &mut Db, seed: u64, rng: &mut SmallRng) -> Result<()> {
    let num_npcs = (WORLD_W * WORLD_H) / 4 * 2;

    for i in 0..num_npcs {
        let npc_seed = seed.wrapping_add(0x4E50_4300 + i as u64);
        let name = names::npc_name(npc_seed);
        let role = random_choice(rng, NPC_ROLES);

        let faction_id: Option<i64> = if rng.gen_bool(0.7) {
            db.conn.query_row(
                "SELECT id FROM factions ORDER BY RANDOM() LIMIT 1",
                [],
                |r| r.get(0),
            ).ok()
        } else {
            None
        };

        let province_id: i64 = db.conn.query_row(
            "SELECT id FROM provinces ORDER BY RANDOM() LIMIT 1",
            [],
            |r| r.get(0),
        )?;

        db.conn.execute(
            "INSERT INTO npcs (name, faction_id, province_id, alive, role, reputation)
             VALUES (?1, ?2, ?3, 1, ?4, ?5)",
            params![name, faction_id, province_id, role, rng.gen_range(-20..20i64)],
        )?;
    }
    Ok(())
}

// ── Static tables ────────────────────────────────────────────────────────────

const BIOMES: &[&str] = &[
    "plains", "forest", "mountain", "swamp", "desert", "coast", "highland", "tundra",
];

const ARCHETYPES: &[&str] = &[
    "guild", "order", "compact", "covenant", "brotherhood",
    "conclave", "circle", "syndicate", "assembly", "league",
];

const NPC_ROLES: &[&str] = &[
    "lord", "merchant", "soldier", "scout", "healer", "scholar",
    "assassin", "wanderer", "bard", "priest", "blacksmith", "thief",
];

fn random_choice<'a>(rng: &mut SmallRng, choices: &[&'a str]) -> &'a str {
    choices[rng.gen_range(0..choices.len())]
}

fn seed_trade_routes(db: &mut Db, rng: &mut SmallRng) -> Result<()> {
    // Create trade routes between random pairs of provinces
    let num_routes = (WORLD_W * WORLD_H) / 3;
    for _ in 0..num_routes {
        let pa: i64 = db.conn.query_row(
            "SELECT id FROM provinces ORDER BY RANDOM() LIMIT 1", [], |r| r.get(0))?;
        let pb: i64 = db.conn.query_row(
            "SELECT id FROM provinces WHERE id != ?1 ORDER BY RANDOM() LIMIT 1",
            params![pa], |r| r.get(0))?;
        db.conn.execute(
            "INSERT OR IGNORE INTO trade_routes (province_a, province_b, disrupted) VALUES (?1, ?2, 0)",
            params![pa, pb])?;
        let _ = rng; // rng available for future use
    }
    Ok(())
}
