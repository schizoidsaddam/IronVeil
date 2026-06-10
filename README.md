# IRONVEIL

> *The machine is the world. Not a metaphor — the simulation state is derived from system state.*

A terminal roguelike where your host machine's runtime is the engine of the world. CPU load drives faction aggression and enemy behavior. RAM pressure shapes the map. Disk I/O triggers earthquakes. Network silence cuts off information and starves provinces. Running Docker containers become named factions with goals, tenets, and memories.

The world runs whether you're watching or not.

---

## What it does

| System metric | World effect |
|---|---|
| CPU 0–30% | Peaceful. Factions recover, morale climbs. |
| CPU 30–60% | Unrest. Bandits on the roads, trade disrupted. |
| CPU 60–80% | Conflict. Skirmishes, border wars, revolt risk climbing. |
| CPU 80%+ | Crisis. Named warlords spawn. Wars declared. Disasters. |
| RAM pressure | Province map complexity (planned) |
| Disk I/O spike | Earthquakes — destabilize provinces, disrupt trade, spread panic via rumors |
| Network silence | Provinces go dark. Rumors freeze. Famine worsens without relief. |
| Swap pressure | Rift events — planar anomalies, morale hits, revolt spikes |
| High thermals | Famine accelerates. Crops fail. |
| Docker containers | Each running container is a faction with a procedurally derived identity |

### Cascade systems

Events compound. A disk spike triggers an earthquake → province destabilizes → famine rises → revolt risk climbs → rumor spawns and travels (degrading accuracy each hop) → rumor arrives distorted → panic spreads → adjacent provinces revolt → power vacuum → aggression factions declare war → war grinds both sides → winner absorbs territory → warlord rises from the chaos. The machine made it happen.

### Factions

Each faction has a procedurally generated identity derived from its name:

- **8 goals**: Commerce, Conquest, Preservation, Purity, Survival, Restoration, Dominion, Ascension
- **5 alignments**: Forthright, Shadow, Ordered, Zealous, Opportunist
- **Distinct voice** for every event type: founding declarations, collapse, civil war, interregnum, stabilization, victory
- **Tenet** — one sentence that defines what they hold above all else

Docker containers map to factions. Container crash = civil war. Restart loop = interregnum. Recovery = stabilization. Each transition writes to the codex with faction-appropriate language.

### The Chronicle

Every significant event writes to `ironveil.chronicle` — a flat text file that persists across sessions and accumulates like sediment. Run `--render-chronicle` to produce a medieval-styled HTML page from it.

---

## Building

Requires Rust (stable). Install via [rustup](https://rustup.rs).

```bash
git clone https://github.com/schizoidsaddam/IronVeil
cd IronVeil
cargo build --release
./target/release/ironveil
```

### Commands

```bash
# Run the game
./target/release/ironveil

# Render chronicle to HTML
./target/release/ironveil --render-chronicle

# Dry run simulation (no TUI, prints all events to stdout)
./target/release/ironveil --dry-run
./target/release/ironveil --dry-run 1000   # specify tick count
```

---

## Controls

```
hjkl / arrow keys   Move
y u b n             Diagonal movement
.  or  Enter        Interact
c                   Toggle Codex panel
Tab                 Cycle panels (Map / Log / Status / Codex)
q                   Quit
```

---

## Architecture

```
src/
  main.rs           Entry point, tokio runtime, input loop
  system/
    metrics.rs      CPU/RAM/swap/disk/network/thermal polling (sysinfo 0.30)
    fingerprint.rs  Machine seed: hostname + CPU brand + RAM → SHA256 → u64
    docker.rs       Docker Unix socket polling (no bollard dep)
  world/
    gen/            World generation — provinces, factions, NPCs, trade routes
    simulation.rs   World tick: revolt, war, famine, rumor propagation, cascade systems
    lore.rs         Procedural faction identity, event language (40+ voice variants)
    names.rs        Phoneme-based procedural names
    state.rs        In-memory game state
    tile.rs         Tile types and glyphs
  data/
    schema.rs       SQLite schema, Db wrapper, codex writer
  render/
    ui.rs           ratatui TUI — map, stats, log, codex panels
  chronicle/        HTML chronicle renderer
  dryrun.rs         Headless simulation harness for testing
```

### Key dependencies

- `ratatui` — TUI rendering
- `sysinfo 0.30` — system metrics
- `rusqlite` (bundled) — world state persistence
- `tokio` — async runtime
- `sha2` — machine fingerprint, deterministic name seeding
- `rand` — seeded procedural generation

---

## World state

The world is always live. No manual save, no save scumming. `ironveil.db` is the world. Delete it to start over with a new world seeded from your machine.

Your machine generates your world, unrepeatable. Same hostname, same CPU, same RAM = same seed = same world.

---

## Status

Early development. The simulation engine is functional. Player agency (interaction, faction influence, rumor injection, holds) is the next major phase.

---

*Callsign: N3JAX*
