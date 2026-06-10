mod chronicle;
mod data;
mod dryrun;
mod render;
mod system;
mod world;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

use data::Db;
use render::Renderer;
use system::SystemPoller;
use world::{GameState, WorldTick};

pub enum AppEvent {
    Tick,
    Key(crossterm::event::KeyEvent),
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("--render-chronicle") => {
            return chronicle::render_html("ironveil.chronicle", "chronicle.html");
        }
        Some("--dry-run") => {
            let ticks: u64 = args.get(2)
                .and_then(|s| s.parse().ok())
                .unwrap_or(500);
            return dryrun::run(ticks).await;
        }
        _ => {}
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    if let Err(ref e) = result { eprintln!("IRONVEIL crashed: {e}"); }
    result
}

async fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let db = Arc::new(Mutex::new(Db::open("ironveil.db")?));

    {
        let mut db_lock = db.lock().unwrap();
        if !db_lock.world_exists()? {
            let seed = system::fingerprint::generate();
            world::gen::generate_world(&mut db_lock, seed)?;
        }
    }

    let game_state = Arc::new(Mutex::new(GameState::load(&db.lock().unwrap())?));

    let (tx, mut rx) = mpsc::unbounded_channel::<AppEvent>();

    let tx_input = tx.clone();
    tokio::spawn(async move {
        loop {
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if tx_input.send(AppEvent::Key(key)).is_err() { break; }
                }
            }
        }
    });

    let tx_tick = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            if tx_tick.send(AppEvent::Tick).is_err() { break; }
        }
    });

    let mut poller        = SystemPoller::new();
    let renderer          = Renderer::new();
    let mut world_tick    = WorldTick::new();
    let mut last_docker   = Instant::now();
    let mut cached_docker = Vec::new();

    loop {
        {
            let state = game_state.lock().unwrap();
            terminal.draw(|f| renderer.draw(f, &state))?;
        }

        match rx.recv().await {
            None => break,

            Some(AppEvent::Key(key)) => {
                if handle_input(key, &game_state, &db)? { break; }
            }

            Some(AppEvent::Tick) => {
                let metrics = poller.poll();

                if last_docker.elapsed() >= Duration::from_secs(10) {
                    cached_docker = system::docker::poll_containers().await.unwrap_or_default();
                    last_docker   = Instant::now();
                }

                let mut state   = game_state.lock().unwrap();
                let mut db_lock = db.lock().unwrap();
                world_tick.tick(&mut state, &metrics, &cached_docker, &mut db_lock)?;
            }
        }
    }

    let state   = game_state.lock().unwrap();
    let db_lock = db.lock().unwrap();
    state.save(&db_lock)?;

    Ok(())
}

fn handle_input(
    key:        crossterm::event::KeyEvent,
    game_state: &Arc<Mutex<GameState>>,
    db:         &Arc<Mutex<Db>>,
) -> Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }

    let mut state = game_state.lock().unwrap();

    match key.code {
        KeyCode::Char('q' | 'Q') => return Ok(true),

        KeyCode::Char('h') | KeyCode::Left  => state.move_player(-1,  0),
        KeyCode::Char('l') | KeyCode::Right => state.move_player( 1,  0),
        KeyCode::Char('k') | KeyCode::Up    => state.move_player( 0, -1),
        KeyCode::Char('j') | KeyCode::Down  => state.move_player( 0,  1),
        KeyCode::Char('y') => state.move_player(-1, -1),
        KeyCode::Char('u') => state.move_player( 1, -1),
        KeyCode::Char('b') => state.move_player(-1,  1),
        KeyCode::Char('n') => state.move_player( 1,  1),

        KeyCode::Char('.') | KeyCode::Enter => {
            let db_lock = db.lock().unwrap();
            state.interact(&db_lock)?;
        }
        KeyCode::Tab       => state.cycle_panel(),
        KeyCode::Char('c') => state.toggle_codex(),
        _ => {}
    }

    Ok(false)
}
