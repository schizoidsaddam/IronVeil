//! Turn-based first-person renderer.
//! Redraws only on player action — no animation, no frame loop.
//! Fakes depth with Unicode block/box characters and character density.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::world::local::{Cell, Facing, LocalMap};
use crate::world::state::GameState;

// Palette
const C_WALL_FAR:    Color = Color::Rgb( 60,  55,  50);
const C_WALL_MID:    Color = Color::Rgb( 90,  80,  70);
#[allow(dead_code)]
const C_WALL_NEAR:   Color = Color::Rgb(120, 110,  95);
const C_FLOOR:       Color = Color::Rgb( 40,  38,  35);
const C_CEILING:     Color = Color::Rgb( 25,  23,  20);
const C_DOOR:        Color = Color::Rgb(120,  90,  40);
const C_WATER:       Color = Color::Rgb( 40,  80, 140);
const C_EXIT:        Color = Color::Rgb( 80, 160,  80);
const C_HOSTILE:     Color = Color::Rgb(200,  50,  50);
const C_FRIENDLY:    Color = Color::Rgb( 80, 180,  80);
const C_ITEM:        Color = Color::Rgb(180, 160,  60);
const C_SWORD:       Color = Color::Rgb(180, 190, 200);
const C_SWORD_HAND:  Color = Color::Rgb(200, 160, 120);
const C_TITLE:       Color = Color::Rgb(180, 140,  60);
const C_DIM:         Color = Color::Rgb( 80,  75,  70);
const C_FG:          Color = Color::Rgb(200, 190, 170);

/// Draw the full first-person viewport into `area`.
pub fn draw_first_person(
    f:       &mut Frame,
    area:    Rect,
    _state:  &GameState,
    local:   &LocalMap,
    facing:  Facing,
    lx:      i32,
    ly:      i32,
    message: Option<&str>,
) {
    let view = local.forward_view(lx, ly, facing);
    let w    = area.width;
    let h    = area.height;

    // Reserve bottom rows: message block (up to 3 lines) + keybind row
    let msg_lines = message.map(|m| {
        // Word-wrap message to viewport width
        wrap_message(m, w.saturating_sub(2) as usize)
    }).unwrap_or_default();
    let msg_rows     = (msg_lines.len() as u16).min(3).max(1);
    let viewport_h   = h.saturating_sub(msg_rows + 2); // +title +keybind

    // Biome wall tint
    let (wall_far, wall_mid) = biome_wall_colors(&local.biome);

    let mut lines: Vec<Line> = Vec::with_capacity(h as usize);

    // Title
    let title = format!(" {} · {} · stability:{} ",
        local.province_name,
        local.biome.to_uppercase(),
        local.stability,
    );
    lines.push(Line::from(Span::styled(
        title, Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD)
    )));

    // Ceiling — biome tinted, with variation
    let ceiling_rows = viewport_h / 4;
    let ceiling_char = ceiling_char_for(&local.biome);
    let ceiling_color = biome_ceiling_color(&local.biome);
    for i in 0..ceiling_rows {
        // Slight gradient — darker at top
        let dim = if i == 0 { 15u8 } else { 22 };
        let c = Color::Rgb(dim, dim.saturating_sub(3), dim.saturating_sub(5));
        let line = mixed_line(w, ceiling_char, '░', c, ceiling_color, i, ceiling_rows);
        lines.push(line);
    }

    // Far wall
    let far_center = view.far[1];
    let far_open   = far_center.passable();
    let far_rows   = viewport_h / 6;
    for row in 0..far_rows {
        lines.push(wall_line(w, far_open, &view.far, row, far_rows,
                             wall_far, &view.npc_far, far_center));
    }

    // Mid wall
    let mid_center = view.mid[1];
    let mid_open   = mid_center.passable();
    let mid_rows   = viewport_h / 5;
    for row in 0..mid_rows {
        lines.push(wall_line(w, mid_open, &view.mid, row, mid_rows,
                             wall_mid, &view.npc_mid, mid_center));
    }

    // Floor — biome tinted, gradient toward player
    let used = ceiling_rows + far_rows + mid_rows;
    let floor_rows = viewport_h.saturating_sub(used);
    let (floor_ch, floor_base) = biome_floor(&local.biome);
    for i in 0..floor_rows {
        #[allow(clippy::cast_possible_truncation)]
        let t = (i * 20 / floor_rows.max(1)) as u8;
        let (r, g, b) = floor_base;
        let color = Color::Rgb(r.saturating_add(t), g.saturating_add(t), b.saturating_add(t));
        lines.push(solid_line(w, floor_ch, color));
    }

    // Item at feet
    if let Some(idx) = local.item_at(lx, ly) {
        let item = &local.items[idx];
        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", item.glyph()), Style::default().fg(C_ITEM).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} on the ground  [.] to take", item.name), Style::default().fg(C_ITEM)),
        ]));
    } else {
        lines.push(Line::from(""));
    }

    // Weapon
    lines.push(weapon_line(w));

    // Message block — multi-line, wrapped
    if msg_lines.is_empty() {
        for _ in 0..msg_rows { lines.push(Line::from("")); }
    } else {
        for ml in &msg_lines {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(ml.clone(), Style::default().fg(C_FG)),
            ]));
        }
        // Pad to msg_rows
        for _ in msg_lines.len()..msg_rows as usize {
            lines.push(Line::from(""));
        }
    }

    // Keybind bar
    let facing_label = facing.label();
    lines.push(Line::from(Span::styled(
        format!(" [w↑] Fwd [s↓] Back [a←] Strafe L [d→] Strafe R [q] Turn L [e] Turn R [.] Act [Esc] Exit  {facing_label}"),
        Style::default().fg(C_DIM),
    )));

    f.render_widget(Paragraph::new(lines), area);
}

// ── Line builders ─────────────────────────────────────────────────────────────

fn solid_line(w: u16, ch: char, color: Color) -> Line<'static> {
    let s: String = std::iter::repeat(ch).take(w as usize).collect();
    Line::from(Span::styled(s, Style::default().fg(color)))
}

/// Build a wall-row line. Handles open corridors, walls, doors, water, exits,
/// and NPC sprites centered in the corridor opening.
fn wall_line(
    w:       u16,
    center_open: bool,
    cells:   &[Cell; 3],
    row:     u16,
    total:   u16,
    color:   Color,
    npc:     &Option<crate::world::local::NpcView>,
    center:  Cell,
) -> Line<'static> {
    let w = w as usize;
    // The corridor opening is the middle third of the width
    let corridor_start = w / 3;
    let corridor_end   = w - w / 3;
    let mid_row        = total / 2;

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(w);

    for col in 0..w {
        let in_corridor = col >= corridor_start && col < corridor_end;
        let ch: char;
        let color_here: Color;

        if !in_corridor {
            // Side walls — always solid, color based on left/right cell
            let side_cell = if col < corridor_start { cells[0] } else { cells[2] };
            (ch, color_here) = wall_char_for(side_cell, color);
        } else if !center_open {
            // Wall ahead — solid center
            (ch, color_here) = wall_char_for(center, color);
        } else {
            // Open corridor — render void/depth, or NPC sprite
            let corridor_col = col - corridor_start;
            let corridor_w   = corridor_end - corridor_start;
            let center_col   = corridor_w / 2;

            if let Some(ref n) = npc {
                // NPC sprite — centered, occupies middle rows
                let sprite = npc_sprite(&n.glyph, corridor_col, center_col, row, mid_row, corridor_w);
                if let Some((sc, scol)) = sprite {
                    ch = sc;
                    color_here = if n.hostile { C_HOSTILE } else { C_FRIENDLY };
                    let _ = scol;
                } else {
                    ch    = ' ';
                    color_here = C_FLOOR;
                }
            } else {
                ch         = ' ';
                color_here = C_FLOOR;
            }
        }

        spans.push(Span::styled(ch.to_string(), Style::default().fg(color_here)));
    }

    Line::from(spans)
}

fn wall_char_for(cell: Cell, base_color: Color) -> (char, Color) {
    match cell {
        Cell::Wall   => ('█', base_color),
        Cell::Door   => ('+', C_DOOR),
        Cell::Water  => ('≈', C_WATER),
        Cell::Exit   => ('◄', C_EXIT),
        Cell::Rubble => ('▒', Color::Rgb(80, 70, 60)),
        Cell::Pillar => ('║', base_color),
        _            => (' ', C_FLOOR),
    }
}

/// Returns Some(char, col) if there should be an NPC character at this cell.
fn npc_sprite(
    glyph:       &char,
    corridor_col: usize,
    center_col:   usize,
    row:          u16,
    mid_row:      u16,
    _corridor_w:  usize,
) -> Option<(char, usize)> {
    // Simple sprite: just the glyph centered
    if corridor_col == center_col && row == mid_row {
        return Some((*glyph, corridor_col));
    }
    // Body around the glyph
    if (corridor_col as i32 - center_col as i32).unsigned_abs() <= 1
        && (row as i32 - mid_row as i32).unsigned_abs() <= 1
    {
        let body_chars = [['╔','╦','╗'],['║',' ','║'],['╚','╩','╝']];
        let dr = (row as i32 - mid_row as i32 + 1) as usize;
        let dc = (corridor_col as i32 - center_col as i32 + 1) as usize;
        if dr < 3 && dc < 3 && !(dr == 1 && dc == 1) {
            return Some((body_chars[dr][dc], corridor_col));
        }
    }
    None
}

fn weapon_line(w: u16) -> Line<'static> {
    let w    = w as usize;
    let center = w / 2;
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(w);

    for col in 0..w {
        let offset = col as i32 - center as i32;
        let ch = match offset {
            -2        => '\\',
            -1        => '\\',
             0        => '|',
             1        => '/',
             2        => '/',
            // Hand grip below blade
            _ if (offset - 3).abs() <= 1 => '▓',
            _ => ' ',
        };
        let color = if (offset).abs() <= 2 { C_SWORD } else if (offset - 3).abs() <= 1 { C_SWORD_HAND } else { C_FLOOR };
        spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
    }

    Line::from(spans)
}

// ── Biome visual helpers ──────────────────────────────────────────────────────

fn biome_wall_colors(biome: &str) -> (Color, Color) {
    match biome {
        "forest" | "swamp"          => (Color::Rgb(35,50,30), Color::Rgb(55,75,45)),
        "mountain" | "highland"     => (Color::Rgb(70,65,60), Color::Rgb(100,95,88)),
        "desert"                    => (Color::Rgb(90,70,40), Color::Rgb(130,100,55)),
        "coast"                     => (Color::Rgb(50,60,75), Color::Rgb(75,88,105)),
        "tundra"                    => (Color::Rgb(65,70,78), Color::Rgb(90,95,105)),
        _                           => (C_WALL_FAR, C_WALL_MID),
    }
}

fn biome_ceiling_color(biome: &str) -> Color {
    match biome {
        "forest" | "swamp" => Color::Rgb(20, 30, 18),
        "desert"           => Color::Rgb(35, 28, 15),
        "coast"            => Color::Rgb(18, 22, 35),
        "tundra"           => Color::Rgb(28, 30, 38),
        _                  => C_CEILING,
    }
}

fn ceiling_char_for(biome: &str) -> char {
    match biome {
        "forest" | "swamp" => '░',
        "mountain"         => '▓',
        "desert"           => '·',
        _                  => '▒',
    }
}

fn biome_floor(biome: &str) -> (char, (u8, u8, u8)) {
    match biome {
        "forest" | "swamp" => ('░', (20, 30, 15)),
        "desert"           => ('·', (45, 35, 18)),
        "mountain"         => ('▒', (35, 32, 28)),
        "coast"            => ('░', (22, 28, 40)),
        "tundra"           => ('·', (32, 35, 42)),
        _                  => ('░', (28, 25, 22)),
    }
}

/// A line mixing two chars — ceiling gradient effect
fn mixed_line(w: u16, ch_a: char, ch_b: char, color_a: Color, color_b: Color, row: u16, total: u16) -> Line<'static> {
    let w = w as usize;
    // Blend ratio shifts as rows increase
    let blend = row * 4 / total.max(1);
    let spans: Vec<Span<'static>> = (0..w).map(|col| {
        let use_a = (col + row as usize) % 4 >= blend as usize;
        let (ch, color) = if use_a { (ch_a, color_a) } else { (ch_b, color_b) };
        Span::styled(ch.to_string(), Style::default().fg(color))
    }).collect();
    Line::from(spans)
}

/// Word-wrap a message to fit within `max_width` chars.
fn wrap_message(msg: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 { return vec![msg.to_string()]; }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in msg.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= max_width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current.clone());
            current = word.to_string();
        }
    }
    if !current.is_empty() { lines.push(current); }
    lines
}

// ── Minimap overlay ───────────────────────────────────────────────────────────

/// Draw a small overhead minimap in the corner of the first-person view.
/// 9×9 cells showing the immediate area around the player.
pub fn draw_minimap(
    f:      &mut Frame,
    area:   Rect,
    local:  &LocalMap,
    lx:     i32,
    ly:     i32,
    facing: Facing,
) {
    let map_w = 19u16; // 9 cells × 2 chars + border
    let map_h = 11u16; // 9 cells + border
    if area.width < map_w || area.height < map_h { return; }

    // Position in top-right of area
    let mx = area.x + area.width - map_w;
    let my = area.y;
    let map_area = Rect { x: mx, y: my, width: map_w, height: map_h };

    let mut lines: Vec<Line> = Vec::with_capacity(map_h as usize);

    for dy in -4i32..=4 {
        let mut spans: Vec<Span<'static>> = vec![
            Span::styled("│".to_string(), Style::default().fg(Color::Rgb(60,55,50))),
        ];
        for dx in -4i32..=4 {
            let cx = lx + dx;
            let cy = ly + dy;
            let (ch, color) = if dx == 0 && dy == 0 {
                // Player — use facing arrow
                let arrow = match facing {
                    Facing::North => '▲', Facing::South => '▼',
                    Facing::East  => '►', Facing::West  => '◄',
                };
                (arrow, Color::Rgb(220, 220, 100))
            } else {
                let cell = local.cell(cx, cy);
                let has_npc  = local.npc_at(cx, cy).is_some();
                let has_item = local.item_at(cx, cy).is_some();
                if has_npc {
                    ('•', Color::Rgb(200, 80, 80))
                } else if has_item {
                    ('·', Color::Rgb(180, 160, 60))
                } else {
                    match cell {
                        Cell::Floor | Cell::Rubble => (' ', Color::Rgb(40,38,35)),
                        Cell::Wall                 => ('▪', Color::Rgb(80,70,60)),
                        Cell::Door                 => ('+', Color::Rgb(120,90,40)),
                        Cell::Water                => ('~', Color::Rgb(40,80,140)),
                        Cell::Exit                 => ('◄', Color::Rgb(80,160,80)),
                        Cell::StairsDown           => ('▼', Color::Rgb(140,100,180)),
                        Cell::Pillar               => ('○', Color::Rgb(80,70,60)),
                    }
                }
            };
            spans.push(Span::styled(format!("{ch} "), Style::default().fg(color)));
        }
        spans.push(Span::styled("│".to_string(), Style::default().fg(Color::Rgb(60,55,50))));
        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines), map_area);
}

#[allow(dead_code)]
/// Draw the interact menu — shown when player presses [.] near something.
pub fn draw_interact_menu(
    f:       &mut Frame,
    area:    Rect,
    options: &[(&str, &str)], // (key, description)
    title:   &str,
) {
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            format!(" {title} "),
            Style::default().fg(C_TITLE).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    for (key, desc) in options {
        lines.push(Line::from(vec![
            Span::styled(format!(" [{key}] "), Style::default().fg(C_TITLE)),
            Span::styled(desc.to_string(), Style::default().fg(C_FG)),
        ]));
    }

    f.render_widget(Paragraph::new(lines), area);
}
