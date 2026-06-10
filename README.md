# IronVeil

Your machine hosts an ancient and dark world.

A terminal-based roguelike game written in Rust where system metrics influence gameplay. Navigate a procedurally generated world, uncover ancient secrets, and survive in a world bound to your machine's hardware state.

## Features

- **Terminal UI**: Beautiful retro terminal graphics powered by Ratatui
- **System Integration**: Real CPU, memory, and Docker container states affect the game world
- **Procedural Generation**: Unique worlds generated from system fingerprints
- **Persistent World**: Game state saved to SQLite database
- **Chronicles**: Record and playback of game history

## Installation

### Prerequisites

- Rust 1.70+ ([Install Rust](https://rustup.rs/))
- Git

### Build from Source

```bash
git clone https://github.com/schizoidsaddam/IronVeil.git
cd IronVeil
cargo build --release
```

The compiled binary will be available at `target/release/ironveil`.

## Running

### Start the game

```bash
cargo run --release
# or if built
./target/release/ironveil
```

### Dry run (no UI, simulates N ticks)

```bash
cargo run --release -- --dry-run 500
```

### Render game chronicle to HTML

```bash
cargo run --release -- --render-chronicle
# Generates chronicle.html from ironveil.chronicle
```

## Controls

| Key | Action |
|-----|--------|
| `h` / `←` | Move left |
| `l` / `→` | Move right |
| `k` / `↑` | Move up |
| `j` / `↓` | Move down |
| `y` | Move up-left |
| `u` | Move up-right |
| `b` | Move down-left |
| `n` | Move down-right |
| `.` / `Enter` | Interact with tile |
| `Tab` | Cycle UI panels |
| `c` | Toggle codex |
| `q` / `Q` | Quit game |
| `Ctrl+C` | Force quit |

## Project Structure

```
src/
├── main.rs          # Entry point and event loop
├── world.rs         # World generation and game state
├── system.rs        # System metrics polling (CPU, memory, Docker)
├── render.rs        # Terminal UI rendering (Ratatui)
├── data.rs          # SQLite database layer
├── chronicle.rs     # Game history recording/playback
└── dryrun.rs        # Headless simulation mode
```

## Game Mechanics

- Your world is generated based on your system's unique fingerprint
- System metrics (CPU usage, memory pressure, running containers) influence world events
- Explore, interact with objects, and uncover the ancient secrets of your machine
- Every session is recorded in the chronicle for playback and analysis

## Development

### Running tests

```bash
cargo test
```

### Running with debug info

```bash
cargo run
```

### Building optimized release

```bash
cargo build --release
```

Release builds include full LTO optimization for best performance.

## Architecture

IronVeil uses:

- **Tokio** for async event handling (non-blocking input, game ticks)
- **Ratatui** + **Crossterm** for terminal UI and raw mode handling
- **Rusqlite** for persistent world state
- **Sysinfo** for system metric polling
- **SHA2** for world generation fingerprinting

The main event loop:
1. Polls system metrics every 2 seconds
2. Processes user input (keyboard)
3. Updates game world state
4. Renders UI to terminal
5. Persists changes to database

## Troubleshooting

**Game crashes with "terminal state corrupted"**
- Kill the process and run `reset` in your terminal to restore normal mode

**World generation takes too long**
- Try `--dry-run` mode to test without rendering

**Docker integration not working**
- Ensure Docker daemon is accessible (check `docker ps` works)

## License

[Add your license here]

## Author

schizoidsaddam

---

*"Your machine hosts an ancient and dark world."*
