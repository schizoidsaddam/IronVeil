//! The Director — builds an itinerary of world events and feeds them
//! to the autopilot as a guided tour of aftermath.
//!
//! When the player returns after a long absence (or any time drift mode
//! engages), the Director reads the DB for recent significant events,
//! constructs a narrative sequence of locations to visit, and annotates
//! each location with ambient dialogue drawn from what actually happened there.

use anyhow::Result;
use rusqlite::params;

use crate::data::Db;

/// A single stop in the director's itinerary.
#[derive(Debug, Clone)]
pub struct DirectorStop {
    /// Overworld coordinates of the province to visit
    pub province_x:   i32,
    pub province_y:   i32,
    pub province_name: String,
    /// Why this province is interesting
    pub reason:       StopReason,
    /// Ambient lines to surface in the viewport while visiting.
    /// Drawn from actual DB events for this province.
    pub ambient:      Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    Revolt,
    Famine,
    War,
    Collapse,
    Warlord,
    ActiveCrisis,
}

impl StopReason {
    pub fn priority(&self) -> u8 {
        match self {
            Self::Collapse     => 5,
            Self::Revolt       => 4,
            Self::War          => 4,
            Self::Warlord      => 3,
            Self::Famine       => 2,
            Self::ActiveCrisis => 1,
        }
    }
}

/// Build a director itinerary from current world state.
/// Returns stops sorted by narrative priority (most dramatic first).
pub fn build_itinerary(db: &Db, current_day: i32) -> Result<Vec<DirectorStop>> {
    let mut stops: Vec<DirectorStop> = Vec::new();

    // 1. Provinces with active revolt or just-revolted (revolt_risk > 70 or stability < 25)
    {
        let mut stmt = db.conn.prepare(
            "SELECT id, name, x, y, stability, revolt_risk, famine
             FROM provinces
             WHERE (revolt_risk > 70 OR stability < 25)
               AND known = 0  -- include unknown provinces for drama
             ORDER BY revolt_risk DESC, stability ASC
             LIMIT 4"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,  // id
                r.get::<_, String>(1)?, // name
                r.get::<_, i32>(2)?,  // x
                r.get::<_, i32>(3)?,  // y
                r.get::<_, i64>(4)?,  // stability
                r.get::<_, i64>(5)?,  // revolt_risk
                r.get::<_, i64>(6)?,  // famine
            ))
        })?;

        for row in rows.flatten() {
            let (pid, name, x, y, stability, _revolt_risk, famine) = row;
            let reason = if stability < 25 { StopReason::Revolt } else { StopReason::ActiveCrisis };
            let ambient = build_ambient_revolt(db, pid, &name, stability, famine, current_day)?;
            stops.push(DirectorStop { province_x: x, province_y: y, province_name: name, reason, ambient });
        }
    }

    // 2. Provinces where a faction collapsed recently (last 10 days)
    {
        let mut stmt = db.conn.prepare(
            "SELECT f.name, p.x, p.y, p.name, p.id
             FROM factions f
             JOIN provinces p ON f.province_id = p.id
             WHERE f.status = 'collapsed'
             LIMIT 3"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?, // faction name
                r.get::<_, i32>(1)?,   // x
                r.get::<_, i32>(2)?,   // y
                r.get::<_, String>(3)?,// province name
                r.get::<_, i64>(4)?,   // province id
            ))
        })?;

        for row in rows.flatten() {
            let (fname, x, y, pname, pid) = row;
            let ambient = build_ambient_collapse(db, pid, &fname, &pname, current_day)?;
            stops.push(DirectorStop {
                province_x: x, province_y: y,
                province_name: pname,
                reason: StopReason::Collapse,
                ambient,
            });
        }
    }

    // 3. Provinces with active wars
    {
        let mut stmt = db.conn.prepare(
            "SELECT p.x, p.y, p.name, p.id,
                    fa.name, fd.name, w.intensity
             FROM wars w
             JOIN provinces p  ON w.province_id  = p.id
             JOIN factions fa ON w.attacker_id  = fa.id
             JOIN factions fd ON w.defender_id  = fd.id
             LIMIT 2"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i32>(0)?,    // x
                r.get::<_, i32>(1)?,    // y
                r.get::<_, String>(2)?, // province name
                r.get::<_, i64>(3)?,    // province id
                r.get::<_, String>(4)?, // attacker name
                r.get::<_, String>(5)?, // defender name
                r.get::<_, i64>(6)?,    // intensity
            ))
        })?;

        for row in rows.flatten() {
            let (x, y, pname, pid, attacker, defender, intensity) = row;
            let ambient = build_ambient_war(db, pid, &pname, &attacker, &defender, intensity, current_day)?;
            stops.push(DirectorStop {
                province_x: x, province_y: y,
                province_name: pname.clone(),
                reason: StopReason::War,
                ambient,
            });
        }
    }

    // 4. Provinces with warlords
    {
        let mut stmt = db.conn.prepare(
            "SELECT n.name, p.x, p.y, p.name, p.id, f.name
             FROM npcs n
             JOIN provinces p  ON n.province_id = p.id
             JOIN factions f   ON n.faction_id  = f.id
             WHERE n.role = 'warlord' AND n.alive = 1
             LIMIT 2"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?, // warlord name
                r.get::<_, i32>(1)?,   // x
                r.get::<_, i32>(2)?,   // y
                r.get::<_, String>(3)?,// province name
                r.get::<_, i64>(4)?,   // province id
                r.get::<_, String>(5)?,// faction name
            ))
        })?;

        for row in rows.flatten() {
            let (wname, x, y, pname, pid, fname) = row;
            let ambient = build_ambient_warlord(db, pid, &wname, &fname, &pname, current_day)?;
            stops.push(DirectorStop {
                province_x: x, province_y: y,
                province_name: pname,
                reason: StopReason::Warlord,
                ambient,
            });
        }
    }

    // 5. Famine provinces not already covered
    {
        let mut stmt = db.conn.prepare(
            "SELECT id, name, x, y, famine
             FROM provinces
             WHERE famine > 55
             LIMIT 2"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i32>(2)?, r.get::<_, i32>(3)?, r.get::<_, i64>(4)?))
        })?;

        for row in rows.flatten() {
            let (pid, pname, x, y, famine) = row;
            // Skip if already in stops
            if stops.iter().any(|s| s.province_x == x && s.province_y == y) { continue; }
            let ambient = build_ambient_famine(db, pid, &pname, famine, current_day)?;
            stops.push(DirectorStop {
                province_x: x, province_y: y,
                province_name: pname,
                reason: StopReason::Famine,
                ambient,
            });
        }
    }

    // Sort by priority — most dramatic first
    stops.sort_by(|a, b| b.reason.priority().cmp(&a.reason.priority()));

    Ok(stops)
}

// ── Ambient dialogue builders ─────────────────────────────────────────────────
// Each builder pulls actual codex/event data for the specific province
// and generates dialogue that references what really happened.

fn build_ambient_revolt(
    db:          &Db,
    _province_id: i64,
    pname:       &str,
    stability:   i64,
    famine:      i64,
    current_day: i32,
) -> Result<Vec<String>> {
    let mut lines: Vec<String> = Vec::new();

    // Pull the most recent revolt/event codex entry for this area
    let recent_event: Option<String> = db.conn.query_row(
        "SELECT entry FROM codex
         WHERE (category = 'revolt' OR category = 'event')
           AND entry LIKE ?1
         ORDER BY id DESC LIMIT 1",
        params![format!("%{pname}%")],
        |r| r.get(0),
    ).ok();

    if famine > 50 {
        lines.push(format!("A survivor: \"The grain stores were empty for weeks before they turned.\""));
    }

    if stability < 15 {
        lines.push(format!("No one is in charge of {pname} anymore. The silence says it all."));
        lines.push("Charred timber where the administration building stood.".into());
    } else if stability < 30 {
        lines.push(format!("{pname} is quiet in the way that means something broke here."));
        lines.push("A rebel banner, hastily made, nailed to a doorframe.".into());
    }

    if let Some(entry) = recent_event {
        // Extract the core event into an ambient observation
        let trimmed = entry
            .split(". ")
            .nth(1)
            .unwrap_or(&entry)
            .trim_end_matches('.')
            .to_string();
        if !trimmed.is_empty() && trimmed.len() < 120 {
            lines.push(format!("You remember the report: \"{trimmed}.\""));
        }
    }

    let day_since = current_day % 7; // rough cycle
    if day_since < 3 {
        lines.push("The fires are recent. The ash is still warm.".into());
    } else {
        lines.push("The fires have been out for some time. The damage remains.".into());
    }

    Ok(lines)
}

fn build_ambient_collapse(
    db:          &Db,
    province_id: i64,
    faction_name: &str,
    pname:        &str,
    current_day:  i32,
) -> Result<Vec<String>> {
    let mut lines: Vec<String> = Vec::new();

    // Pull collapse codex entry
    let collapse_entry: Option<String> = db.conn.query_row(
        "SELECT entry FROM codex
         WHERE category = 'faction'
           AND entry LIKE ?1
         ORDER BY id DESC LIMIT 1",
        params![format!("%{}%", faction_name)],
        |r| r.get(0),
    ).ok();

    lines.push(format!("{faction_name} is gone. Their halls stand empty."));

    if let Some(entry) = collapse_entry {
        let core = entry.split(". ").last().unwrap_or("").trim_end_matches('.').to_string();
        if !core.is_empty() && core.len() < 100 {
            lines.push(format!("A survivor says: \"{core}\""));
        }
    }

    // Check if another faction moved in
    let new_faction: Option<String> = db.conn.query_row(
        "SELECT name FROM factions
         WHERE province_id = ?1 AND status = 'stable'
         LIMIT 1",
        params![province_id],
        |r| r.get(0),
    ).ok();

    if let Some(fname) = new_faction {
        lines.push(format!("{fname} has moved into the void. Their flags are new. Their intentions are not."));
    } else {
        lines.push(format!("No one has claimed {pname} yet. The hesitation is telling."));
    }

    let _ = current_day;
    Ok(lines)
}

fn build_ambient_war(
    db:          &Db,
    province_id: i64,
    pname:       &str,
    attacker:    &str,
    defender:    &str,
    intensity:   i64,
    current_day: i32,
) -> Result<Vec<String>> {
    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("The war between {attacker} and {defender} is still being fought here."));

    if intensity > 70 {
        lines.push(format!("{pname} is a contested ruin. Both sides are bleeding."));
        lines.push("A soldier, neither's colors: \"There are no sides anymore. Just survivors.\"".into());
    } else if intensity > 40 {
        lines.push(format!("Skirmish lines run through {pname}. The outcome is not decided."));
        lines.push(format!("A {attacker} soldier at a barricade. He doesn't ask your allegiance."));
    } else {
        lines.push(format!("The fighting here has slowed. Exhaustion more than strategy."));
    }

    // Pull war codex entry
    let war_entry: Option<String> = db.conn.query_row(
        "SELECT entry FROM codex
         WHERE category = 'war'
           AND (entry LIKE ?1 OR entry LIKE ?2)
         ORDER BY id DESC LIMIT 1",
        params![format!("%{attacker}%"), format!("%{defender}%")],
        |r| r.get(0),
    ).ok();

    if let Some(entry) = war_entry {
        let day_str = entry.split(". ").next().unwrap_or("").to_string();
        if !day_str.is_empty() {
            lines.push(format!("From the codex: {day_str}."));
        }
    }

    let _ = (province_id, current_day);
    Ok(lines)
}

fn build_ambient_warlord(
    db:           &Db,
    province_id:  i64,
    warlord_name: &str,
    faction_name: &str,
    pname:        &str,
    current_day:  i32,
) -> Result<Vec<String>> {
    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("{warlord_name} is here. You can feel it before you see it."));
    lines.push(format!("{faction_name}'s soldiers hold every approach."));

    // Pull warlord codex entry
    let warlord_entry: Option<String> = db.conn.query_row(
        "SELECT entry FROM codex
         WHERE category = 'named_entity'
           AND entry LIKE ?1
         ORDER BY id DESC LIMIT 1",
        params![format!("%{warlord_name}%")],
        |r| r.get(0),
    ).ok();

    if let Some(entry) = warlord_entry {
        // Extract the tenet line
        if let Some(tenet_start) = entry.find("Their tenet:") {
            let tenet = entry[tenet_start..].trim_end_matches('"').trim_end_matches('.');
            lines.push(format!("Scratched into the wall near the gate: {tenet}"));
        }
    }

    lines.push(format!("{pname} answers to {warlord_name} now. Ask a local if you doubt it."));

    let _ = (province_id, current_day);
    Ok(lines)
}

fn build_ambient_famine(
    db:          &Db,
    province_id: i64,
    pname:       &str,
    famine:      i64,
    current_day: i32,
) -> Result<Vec<String>> {
    let mut lines: Vec<String> = Vec::new();

    if famine > 80 {
        lines.push(format!("The market in {pname} is closed. Has been for days."));
        lines.push("A woman at a doorway: \"We sent the children to relatives. The relatives sent them back.\"".into());
        lines.push("The granary doors hang open. Whatever was inside is gone.".into());
    } else if famine > 60 {
        lines.push(format!("{pname}: the food prices are posted publicly now. It doesn't help."));
        lines.push("People are leaving. The roads south are crowded.".into());
    }

    // Pull famine codex entry
    let famine_entry: Option<String> = db.conn.query_row(
        "SELECT entry FROM codex
         WHERE category = 'event'
           AND entry LIKE ?1
           AND entry LIKE '%starv%'
         ORDER BY id DESC LIMIT 1",
        params![format!("%{pname}%")],
        |r| r.get(0),
    ).ok();

    if let Some(entry) = famine_entry {
        let core = entry.split(". ").nth(1).unwrap_or("").trim_end_matches('.').to_string();
        if !core.is_empty() && core.len() < 100 {
            lines.push(format!("Someone wrote on a wall: \"{core}\""));
        }
    }

    // Check if trade disrupted
    let disrupted: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM trade_routes
         WHERE (province_a = ?1 OR province_b = ?1) AND disrupted = 1",
        params![province_id],
        |r| r.get(0),
    ).unwrap_or(0);

    if disrupted > 0 {
        lines.push("The trade road is blocked. Has been for some time.".into());
    }

    let _ = current_day;
    Ok(lines)
}
