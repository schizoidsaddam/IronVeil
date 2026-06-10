//! `--dry-run [N]` mode: exercises the full simulation loop N ticks without TUI.

use anyhow::Result;

use crate::data::Db;
use crate::system::{SystemMetrics, SystemPoller, WorldTension};
use crate::system::docker::{ContainerInfo, ContainerStatus};
use crate::world::{GameState, WorldTick};

pub async fn run(ticks: u64) -> Result<()> {
    println!("━━━ IRONVEIL DRY RUN ({ticks} ticks) ━━━");
    println!("Day advances every 30 ticks → {} days simulated", ticks / 30);
    println!();

    let mut db   = Db::open(":memory:")?;
    let seed     = crate::system::fingerprint::generate();
    println!("Machine seed: {seed:#018x}");
    crate::world::gen::generate_world(&mut db, seed)?;
    println!("World generated OK.");
    println!();

    let mut state      = GameState::load(&db)?;
    let mut poller     = SystemPoller::new();
    let mut world_tick = WorldTick::new();

    let base_docker: Vec<ContainerInfo> = vec![
        mk("sonarr",      ContainerStatus::Running),
        mk("radarr",      ContainerStatus::Running),
        mk("jellyfin",    ContainerStatus::Running),
        mk("qbittorrent", ContainerStatus::Running),
        mk("prowlarr",    ContainerStatus::Running),
    ];
    // Pre-built override vecs — must outlive the tick loop
    let docker_crash   = override_one(&base_docker, "jellyfin", ContainerStatus::Restarting);
    let docker_recover = base_docker.clone();

    // Injection schedule — all tick-aligned to multiples of 5 (docker sync rate)
    // and to rift window (multiples of 15).
    let tick_docker_crash   = 100u64; // multiple of 5
    let tick_docker_recover = 115u64; // multiple of 5
    let tick_disk_spike     =  45u64; // before quake cooldown expires
    let tick_rift           =  75u64; // multiple of 15, swap injected here
    let tick_silence_start  = 200u64;
    let tick_silence_end    = 260u64; // silence lasts 60 ticks = 3 isolation events
    let tick_move_player    = 120u64; // move player so fatigue accumulates

    let mut prev_log_len   = 0usize;
    let mut prev_codex_len = 0usize;
    let mut prev_day       = state.world_day;
    let mut errors         = 0u32;

    for tick in 1..=ticks {
        let mut metrics = poller.poll();

        // Ramp CPU through all tension tiers
        metrics = synthetic_stress(metrics, tick, ticks);

        // Inject scenarios
        if tick == tick_disk_spike {
            println!("[tick {tick}] INJECT: disk spike → earthquake");
            metrics.disk.spike = true;
        }
        if tick == tick_rift {
            println!("[tick {tick}] INJECT: swap pressure 0.72 → rift (fires at next tick%15==0)");
            metrics.swap_pressure = 0.72;
        }
        if (tick_silence_start..=tick_silence_end).contains(&tick) {
            metrics.network.silent = true;
            // Also zero the streak so hysteresis is bypassed for testing
        }
        if tick == tick_move_player {
            println!("[tick {tick}] INJECT: player moves 10 steps");
            for _ in 0..10 { state.move_player(1, 0); }
        }

        // Docker: only feed on multiples of 5
        let docker_slice: &[ContainerInfo] = if tick % 5 != 0 {
            &[]
        } else if tick == tick_docker_crash {
            println!("[tick {tick}] INJECT: jellyfin → RESTARTING");
            &docker_crash
        } else if tick == tick_docker_recover {
            println!("[tick {tick}] INJECT: jellyfin → RUNNING (recovered)");
            &docker_recover
        } else {
            &base_docker
        };

        if let Err(e) = world_tick.tick(&mut state, &metrics, docker_slice, &mut db) {
            eprintln!("[tick {tick}] ERROR: {e}");
            errors += 1;
        }

        if state.world_day != prev_day {
            println!(
                "\n── Day {} (tick {tick})  tension={}  hp={}  hunger={}  fatigue={}  dark={}  pos=({},{})",
                state.world_day, state.tension.label(),
                state.player.hp, state.player.hunger, state.player.fatigue,
                state.provinces_dark, state.player.x, state.player.y,
            );
            prev_day = state.world_day;
        }

        // Log may drain 20 entries at once when capped — clamp prev_log_len defensively
        let log_start = prev_log_len.min(state.log.len());
        for entry in &state.log[log_start..] {
            println!("  LOG   [{}] {}", entry.day, entry.message);
        }
        prev_log_len = state.log.len();

        // Codex is prepended — new entries sit at the front.
        // When cap truncates, len can shrink; saturating_sub handles that.
        let new_codex = state.codex.len().saturating_sub(prev_codex_len);
        for entry in state.codex.iter().take(new_codex) {
            println!("  CODEX [day {}] ({}) {}", entry.day, entry.category, entry.text);
        }
        prev_codex_len = state.codex.len();
    }

    println!();
    println!("━━━ FINAL STATE ━━━");
    println!("World day:      {}", state.world_day);
    println!("Player HP:      {}/{}", state.player.hp, state.player.max_hp);
    println!("Player hunger:  {}", state.player.hunger);
    println!("Player fatigue: {}", state.player.fatigue);
    println!("Dark provinces: {}", state.provinces_dark);
    println!("Tension:        {}", state.tension.label());
    println!("Log entries:    {}", state.log.len());
    println!("Codex entries:  {}", state.codex.len());
    println!();

    println!("━━━ DB INTEGRITY ━━━");
    run_checks(&db, errors)
}

fn run_checks(db: &Db, errors: u32) -> Result<()> {
    let provinces:       i64 = q(db, "SELECT COUNT(*) FROM provinces")?;
    let factions:        i64 = q(db, "SELECT COUNT(*) FROM factions")?;
    let collapsed:       i64 = q(db, "SELECT COUNT(*) FROM factions WHERE status='collapsed'")?;
    let npcs:            i64 = q(db, "SELECT COUNT(*) FROM npcs")?;
    let warlords:        i64 = q(db, "SELECT COUNT(*) FROM npcs WHERE role='warlord'")?;
    let codex:           i64 = q(db, "SELECT COUNT(*) FROM codex")?;
    let dark:            i64 = q(db, "SELECT COUNT(*) FROM provinces WHERE dark=1")?;
    let player_ok:      bool = q(db, "SELECT COUNT(*) FROM player WHERE id=1")? == 1i64;
    let orphans:         i64 = q(db,
        "SELECT COUNT(*) FROM factions f \
         WHERE province_id IS NOT NULL \
           AND NOT EXISTS (SELECT 1 FROM provinces p WHERE p.id=f.province_id)")?;
    let container_factions: i64 = q(db,
        "SELECT COUNT(*) FROM factions WHERE container_name IS NOT NULL")?;

    println!("Provinces:          {provinces} (expect 192)");
    println!("Factions:           {factions} ({collapsed} collapsed, {container_factions} from containers)");
    println!("NPCs:               {npcs} ({warlords} warlords)");
    println!("Codex entries:      {codex}");
    println!("Dark provinces:     {dark}");
    println!("Player row:         {player_ok}");
    println!("Orphaned factions:  {orphans}");
    println!();

    let mut failed = errors;

    macro_rules! chk {
        ($cond:expr, $label:expr) => {
            if $cond {
                println!("  OK  {}", $label);
            } else {
                eprintln!("  FAIL {}", $label);
                failed += 1;
            }
        };
    }

    chk!(provinces == 192,          "province count == 192");
    chk!(factions > 0,              "factions exist");
    chk!(player_ok,                 "player row exists");
    chk!(codex > 0,                 "codex has entries");
    chk!(orphans == 0,              "no orphaned factions");
    chk!(provinces - dark > 0,      "at least one lit province");
    chk!(container_factions > 0,   "container→faction sync fired");

    println!();
    if failed == 0 {
        println!("✓ All checks passed. {}", if errors == 0 { "Clean run." } else { "" });
    } else {
        eprintln!("✗ {failed} check(s) FAILED.");
        std::process::exit(1);
    }
    Ok(())
}

fn q(db: &Db, sql: &str) -> Result<i64> {
    Ok(db.conn.query_row(sql, [], |r| r.get(0))?)
}

fn synthetic_stress(mut m: SystemMetrics, tick: u64, total: u64) -> SystemMetrics {
    let q = (total / 4).max(1);
    m.cpu_pct = match tick / q {
        0 => 15.0,
        1 => 45.0,
        2 => 70.0,
        _ => 85.0,
    };
    m.tension = WorldTension::from_cpu(m.cpu_pct);
    m
}

fn mk(name: &str, status: ContainerStatus) -> ContainerInfo {
    ContainerInfo {
        id:     name[..name.len().min(12)].to_string(),
        name:   name.to_string(),
        image:  format!("{name}:latest"),
        status,
    }
}

fn override_one(base: &[ContainerInfo], target: &str, status: ContainerStatus) -> Vec<ContainerInfo> {
    base.iter().map(|c| {
        if c.name == target { ContainerInfo { status: status.clone(), ..c.clone() } }
        else                { c.clone() }
    }).collect()
}
