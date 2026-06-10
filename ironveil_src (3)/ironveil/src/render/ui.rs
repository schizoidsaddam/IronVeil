use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::world::GameState;
use crate::world::state::Panel;
use crate::system::WorldTension;

const C_FG:               Color = Color::Rgb(200, 190, 170);
const C_DIM:              Color = Color::Rgb(100,  95,  85);
const C_BORDER:           Color = Color::Rgb( 80,  70,  60);
const C_TITLE:            Color = Color::Rgb(180, 140,  60);
const C_DANGER:           Color = Color::Rgb(200,  50,  50);
const C_TENSION_PEACEFUL: Color = Color::Rgb( 80, 160,  80);
const C_TENSION_UNREST:   Color = Color::Rgb(200, 180,  60);
const C_TENSION_CONFLICT: Color = Color::Rgb(200, 120,  40);
const C_TENSION_CRISIS:   Color = Color::Rgb(200,  50,  50);
const C_PLAYER:           Color = Color::Rgb(220, 220, 100);
const C_MAP_BG:           Color = Color::Rgb( 20,  18,  15);
const C_CODEX_CONFLICT:   Color = Color::Rgb(180,  80,  60);
const C_CODEX_OMEN:       Color = Color::Rgb(140, 100, 180);
const C_CODEX_FACTION:    Color = Color::Rgb(100, 160, 200);
const C_CODEX_NAMED:      Color = Color::Rgb(220, 180,  80);

/// Stateless renderer — all drawing logic is free functions.
pub struct Renderer;

impl Renderer {
    pub fn new() -> Self { Self }

    pub fn draw(&self, f: &mut Frame, state: &GameState) {
        let area = f.size();

        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(area);

        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(40), Constraint::Length(30)])
            .split(root[0]);

        let sidebar = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(14), Constraint::Min(0)])
            .split(body[1]);

        match state.active_panel {
            Panel::Codex => draw_codex(f, body[0], state),
            _            => draw_map(f, body[0], state),
        }

        draw_stats(f, sidebar[0], state);
        draw_log(f, sidebar[1], state);
        draw_status_bar(f, root[1], state);
    }
}

// ── Panels ────────────────────────────────────────────────────────────────────

fn draw_map(f: &mut Frame, area: Rect, state: &GameState) {
    let title = format!(" IRONVEIL — Day {} ", state.world_day);
    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_BORDER))
        .style(Style::default().bg(C_MAP_BG));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let cx = i32::from(inner.width)  / 2;
    let cy = i32::from(inner.height) / 2;

    let lines: Vec<Line> = (0..inner.height)
        .map(|row| {
            let spans: Vec<Span> = (0..inner.width)
                .map(|col| {
                    let dx = i32::from(col) - cx;
                    let dy = i32::from(row) - cy;
                    if dx == 0 && dy == 0 {
                        Span::styled("@", Style::default().fg(C_PLAYER).add_modifier(Modifier::BOLD))
                    } else {
                        let ch = procedural_tile(state.player.x + dx, state.player.y + dy);
                        Span::styled(String::from(ch), Style::default().fg(tile_color(ch)))
                    }
                })
                .collect();
            Line::from(spans)
        })
        .collect();

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_codex(f: &mut Frame, area: Rect, state: &GameState) {
    let block = Block::default()
        .title(Span::styled(" CODEX ", Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(C_BORDER));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if state.codex.is_empty() {
        f.render_widget(
            Paragraph::new("No entries yet.")
                .style(Style::default().fg(C_DIM)),
            inner,
        );
        return;
    }

    let inner_height = usize::from(inner.height);

    let items: Vec<ListItem> = state
        .codex
        .iter()
        .take(inner_height)
        .map(|e| {
            let day_span = Span::styled(
                format!("[Day {}] ", e.day),
                Style::default().fg(C_DIM),
            );
            let cat_color = codex_category_color(&e.category);
            let cat_span  = Span::styled(
                format!("({}) ", e.category),
                Style::default().fg(cat_color),
            );
            let text_span = Span::styled(e.text.clone(), Style::default().fg(C_FG));
            ListItem::new(Line::from(vec![day_span, cat_span, text_span]))
        })
        .collect();

    f.render_widget(
        List::new(items).block(Block::default()),
        inner,
    );
}

fn draw_stats(f: &mut Frame, area: Rect, state: &GameState) {
    let tension_color = tension_display_color(state.tension);
    let tension_label = state.tension.label();
    let hp_color      = if state.player.hp < state.player.max_hp / 3 { C_DANGER } else { C_FG };

    let (dark_label, dark_color) = if state.provinces_dark > 0 {
        (format!("{} DARK", state.provinces_dark), C_DANGER)
    } else {
        ("none".into(), C_DIM)
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(
                state.player.name.clone(),
                Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  Gen.{}", state.player.generation),
                Style::default().fg(C_DIM),
            ),
        ]),
        Line::from(""),
        stat_line("HP     ", format!("{}/{}", state.player.hp, state.player.max_hp), hp_color),
        stat_line("HUNGER ", format!("{}", state.player.hunger), hunger_color(state.player.hunger)),
        stat_line("FATIGUE", format!("{}", state.player.fatigue), C_FG),
        Line::from(""),
        stat_line("POS    ", format!("{},{}", state.player.x, state.player.y), C_FG),
        Line::from(""),
        stat_line("WORLD  ", tension_label.to_string(), tension_color),
        Line::from(""),
        stat_line("DARK   ", dark_label, dark_color),
    ];

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(Span::styled(" STATUS ", Style::default().fg(C_TITLE)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(C_BORDER)),
        ),
        area,
    );
}

fn draw_log(f: &mut Frame, area: Rect, state: &GameState) {
    let inner_height = usize::from(area.height.saturating_sub(2));

    let items: Vec<ListItem> = state
        .log
        .iter()
        .rev()
        .take(inner_height)
        .map(|e| ListItem::new(Line::from(vec![
            Span::styled(format!("[{}] ", e.day), Style::default().fg(C_DIM)),
            Span::styled(e.message.clone(), Style::default().fg(C_FG)),
        ])))
        .collect();

    f.render_widget(
        List::new(items).block(
            Block::default()
                .title(Span::styled(" LOG ", Style::default().fg(C_TITLE)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(C_BORDER)),
        ),
        area,
    );
}

fn draw_status_bar(f: &mut Frame, area: Rect, state: &GameState) {
    // Context-sensitive hint: show current panel in bar
    let panel_label = match state.active_panel {
        Panel::Map    => "MAP",
        Panel::Log    => "LOG",
        Panel::Status => "STATUS",
        Panel::Codex  => "CODEX",
    };
    let hint = format!(
        " [hjkl/↑↓←→] Move  [.] Act  [c] Codex  [Tab] Panel:{panel_label}  [q] Quit"
    );
    f.render_widget(
        Paragraph::new(hint).style(Style::default().fg(C_DIM).bg(Color::Rgb(15, 12, 10))),
        area,
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn stat_line(label: &'static str, value: String, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(label, Style::default().fg(C_DIM)),
        Span::styled(format!(" {value}"), Style::default().fg(color)),
    ])
}

fn procedural_tile(x: i32, y: i32) -> char {
    let h = x.wrapping_mul(73_856_093).wrapping_add(y.wrapping_mul(19_349_663));
    match (h as u32) % 20 {
        0..=10 => '.',
        11..=13 => '♣',
        14..=15 => '~',
        16     => '▲',
        17     => '%',
        18     => '═',
        _      => '#',
    }
}

fn tile_color(ch: char) -> Color {
    match ch {
        '.' => Color::Rgb( 60,  80, 40),
        '♣' => Color::Rgb( 30,  90, 30),
        '~' => Color::Rgb( 30,  60,130),
        '▲' => Color::Rgb(120, 110,100),
        '%' => Color::Rgb( 60,  80, 50),
        '═' => Color::Rgb(100,  90, 70),
        '#' => Color::Rgb( 80,  70, 60),
        _   => Color::Rgb( 50,  50, 50),
    }
}

fn tension_display_color(tension: WorldTension) -> Color {
    match tension {
        WorldTension::Peaceful => C_TENSION_PEACEFUL,
        WorldTension::Unrest   => C_TENSION_UNREST,
        WorldTension::Conflict => C_TENSION_CONFLICT,
        WorldTension::Crisis   => C_TENSION_CRISIS,
    }
}

fn codex_category_color(category: &str) -> Color {
    match category {
        "conflict"     => C_CODEX_CONFLICT,
        "omen"         => C_CODEX_OMEN,
        "faction"      => C_CODEX_FACTION,
        "named_entity" => C_CODEX_NAMED,
        _              => C_DIM,
    }
}

fn hunger_color(hunger: i32) -> Color {
    if hunger < 20 { C_DANGER } else if hunger < 50 { C_TENSION_CONFLICT } else { C_FG }
}
