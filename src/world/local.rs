//! Local map — the tile grid you walk through when inside a province.
//! Generated procedurally from the province's biome + stability seed.
//! Not stored in SQLite — regenerated on entry, consistent via seed.

use rand::{Rng, SeedableRng};
use rand::rngs::SmallRng;

pub const LOCAL_W: usize = 24;
pub const LOCAL_H: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    Floor,
    Wall,
    Door,       // passable, blocks sight
    Water,
    Rubble,     // passable, impassable if stability < 20
    Pillar,     // impassable, decorative
    StairsDown, // dungeon entrance
    Exit,       // leave the local map back to overworld
}

impl Cell {
    pub fn passable(self) -> bool {
        matches!(self, Cell::Floor | Cell::Door | Cell::Rubble | Cell::StairsDown | Cell::Exit)
    }

    #[allow(dead_code)]
    pub fn blocks_sight(self) -> bool {
        matches!(self, Cell::Wall | Cell::Pillar | Cell::Door)
    }

    #[allow(dead_code)]
    pub fn glyph(self) -> char {
        match self {
            Cell::Floor      => '.',
            Cell::Wall       => '█',
            Cell::Door       => '+',
            Cell::Water      => '~',
            Cell::Rubble     => '░',
            Cell::Pillar     => '○',
            Cell::StairsDown => '▼',
            Cell::Exit       => '◄',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing { North, East, South, West }

impl Facing {
    pub fn left(self) -> Self {
        match self { Self::North => Self::West, Self::West => Self::South,
                     Self::South => Self::East, Self::East => Self::North }
    }
    pub fn right(self) -> Self {
        match self { Self::North => Self::East, Self::East => Self::South,
                     Self::South => Self::West, Self::West => Self::North }
    }
    pub fn delta(self) -> (i32, i32) {
        match self { Self::North => (0,-1), Self::East => (1,0),
                     Self::South => (0,1),  Self::West => (-1,0) }
    }
    pub fn label(self) -> &'static str {
        match self { Self::North => "N", Self::East => "E",
                     Self::South => "S", Self::West => "W" }
    }
}

/// What the player can see in the 3×3 forward cone.
/// Used by the first-person renderer.
#[derive(Debug, Clone)]
pub struct ForwardView {
    /// Far wall (2 steps ahead) — left, center, right
    pub far:  [Cell; 3],
    /// Mid wall (1 step ahead) — left, center, right  
    pub mid:  [Cell; 3],
    /// Immediate cell (0 steps, player's feet) — left, center, right
    pub near: [Cell; 3],
    /// Whether there's an NPC in the center far cell
    pub npc_far:  Option<NpcView>,
    /// Whether there's an NPC in the center mid cell
    pub npc_mid:  Option<NpcView>,
}

#[derive(Debug, Clone)]
pub struct NpcView {
    pub glyph:  char,
    pub color:  (u8, u8, u8),
    pub name:   String,
    pub hostile: bool,
}

pub struct LocalMap {
    pub cells:      [[Cell; LOCAL_W]; LOCAL_H],
    pub province_name: String,
    pub biome:      String,
    pub stability:  i64,
    /// NPCs present in this local map
    pub npcs:       Vec<LocalNpc>,
    /// Items on the ground
    pub items:      Vec<LocalItem>,
}

#[derive(Debug, Clone)]
pub struct LocalNpc {
    pub x:       usize,
    pub y:       usize,
    pub name:    String,
    pub role:    String,
    pub hp:      i32,
    pub hostile: bool,
    pub alive:   bool,
    pub db_id:   i64,
}

#[derive(Debug, Clone)]
pub struct LocalItem {
    pub x:    usize,
    pub y:    usize,
    pub name: String,
    pub kind: ItemKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Food,
    Weapon,
    Armor,
    Gold,
    Scroll,
}

impl LocalItem {
    #[allow(dead_code)]
    pub fn glyph(&self) -> char {
        match self.kind {
            ItemKind::Food   => '%',
            ItemKind::Weapon => '/',
            ItemKind::Armor  => '[',
            ItemKind::Gold   => '$',
            ItemKind::Scroll => '?',
        }
    }
}

impl LocalMap {
    /// Generate a local map from province data.
    /// Seed = province_id XOR stability so same province always generates same map.
    pub fn generate(
        province_id:   i64,
        province_name: String,
        biome:         String,
        stability:     i64,
        npcs:          Vec<(i64, String, String)>, // (id, name, role)
    ) -> Self {
        let seed = (province_id as u64).wrapping_mul(0x9E3779B9)
            ^ (stability as u64).wrapping_mul(0x517CC1B7);
        let mut rng = SmallRng::seed_from_u64(seed);

        let mut cells = [[Cell::Wall; LOCAL_W]; LOCAL_H];

        // Generate layout based on biome
        match biome.as_str() {
            "plains" | "coast" => gen_open(&mut cells, &mut rng),
            "forest" | "swamp" => gen_forest(&mut cells, &mut rng),
            "mountain" | "highland" | "tundra" => gen_ruins(&mut cells, &mut rng),
            "desert" => gen_desert(&mut cells, &mut rng),
            _ => gen_town(&mut cells, &mut rng),
        }

        // Always carve an exit on the south edge center
        cells[LOCAL_H - 1][LOCAL_W / 2] = Cell::Exit;
        cells[LOCAL_H - 2][LOCAL_W / 2] = Cell::Floor;

        // Place stairs in unstable provinces
        if stability < 40 && rng.gen_bool(0.6) {
            let sx = rng.gen_range(2..LOCAL_W - 2);
            let sy = rng.gen_range(2..LOCAL_H - 2);
            if cells[sy][sx] == Cell::Floor {
                cells[sy][sx] = Cell::StairsDown;
            }
        }

        // Rubble in low-stability provinces
        if stability < 50 {
            let rubble_count = ((50 - stability) / 5) as usize;
            for _ in 0..rubble_count {
                let rx = rng.gen_range(1..LOCAL_W - 1);
                let ry = rng.gen_range(1..LOCAL_H - 1);
                if cells[ry][rx] == Cell::Floor {
                    cells[ry][rx] = Cell::Rubble;
                }
            }
        }

        // Place NPCs from the DB list
        let mut local_npcs: Vec<LocalNpc> = npcs.into_iter().filter_map(|(id, name, role)| {
            for _ in 0..20 {
                let nx = rng.gen_range(1..LOCAL_W - 1);
                let ny = rng.gen_range(1..LOCAL_H - 1);
                if cells[ny][nx] == Cell::Floor {
                    let hostile = matches!(role.as_str(), "warlord" | "soldier" | "assassin" | "thief")
                        && stability < 50;
                    return Some(LocalNpc { x: nx, y: ny, name, role, hp: 20, hostile, alive: true, db_id: id });
                }
            }
            None
        }).collect();

        // Spawn 1-3 hostile encounters in unstable provinces even without DB NPCs
        if stability < 35 {
            let count = rng.gen_range(1..=3usize);
            for _ in 0..count {
                for _ in 0..20 {
                    let nx = rng.gen_range(1..LOCAL_W - 1);
                    let ny = rng.gen_range(1..LOCAL_H - 1);
                    if cells[ny][nx] == Cell::Floor {
                        local_npcs.push(LocalNpc {
                            x: nx, y: ny,
                            name: "Insurgent".into(),
                            role: "soldier".into(),
                            hp: 15, hostile: true, alive: true, db_id: -1,
                        });
                        break;
                    }
                }
            }
        }

        // Scatter food items — more scarce in famine provinces
        let item_count = rng.gen_range(1..=4usize);
        let mut items: Vec<LocalItem> = Vec::new();
        for _ in 0..item_count {
            for _ in 0..20 {
                let ix = rng.gen_range(1..LOCAL_W - 1);
                let iy = rng.gen_range(1..LOCAL_H - 1);
                if cells[iy][ix] == Cell::Floor {
                    let kind = match rng.gen_range(0..5u8) {
                        0 => ItemKind::Food,
                        1 => ItemKind::Weapon,
                        2 => ItemKind::Armor,
                        3 => ItemKind::Gold,
                        _ => ItemKind::Scroll,
                    };
                    items.push(LocalItem { x: ix, y: iy, name: item_name(kind, &mut rng).into(), kind });
                    break;
                }
            }
        }

        Self { cells, province_name, biome, stability, npcs: local_npcs, items }
    }

    pub fn cell(&self, x: i32, y: i32) -> Cell {
        if x < 0 || y < 0 || x >= LOCAL_W as i32 || y >= LOCAL_H as i32 {
            return Cell::Wall;
        }
        self.cells[y as usize][x as usize]
    }

    pub fn npc_at(&self, x: i32, y: i32) -> Option<&LocalNpc> {
        self.npcs.iter().find(|n| n.alive && n.x == x as usize && n.y == y as usize)
    }

    pub fn item_at(&self, x: i32, y: i32) -> Option<usize> {
        self.items.iter().position(|i| i.x == x as usize && i.y == y as usize)
    }

    /// Build the forward view from a position + facing for the first-person renderer.
    pub fn forward_view(&self, px: i32, py: i32, facing: Facing) -> ForwardView {
        let (fdx, fdy) = facing.delta();
        let (rdx, rdy) = facing.right().delta();

        // Cells at distance 1, 2 ahead — and one step left/right at each
        let view_cell = |fwd: i32, side: i32| -> Cell {
            self.cell(px + fdx * fwd + rdx * side, py + fdy * fwd + rdy * side)
        };

        let npc_at_fwd = |fwd: i32| -> Option<NpcView> {
            let cx = px + fdx * fwd;
            let cy = py + fdy * fwd;
            self.npc_at(cx, cy).map(|n| NpcView {
                glyph:   npc_glyph(&n.role),
                color:   if n.hostile { (200, 60, 60) } else { (100, 180, 100) },
                name:    n.name.clone(),
                hostile: n.hostile,
            })
        };

        ForwardView {
            far:     [view_cell(2, -1), view_cell(2, 0), view_cell(2, 1)],
            mid:     [view_cell(1, -1), view_cell(1, 0), view_cell(1, 1)],
            near:    [view_cell(0, -1), view_cell(0, 0), view_cell(0, 1)],
            npc_far: npc_at_fwd(2),
            npc_mid: npc_at_fwd(1),
        }
    }
}

// ── Map generators ────────────────────────────────────────────────────────────

fn gen_town(cells: &mut [[Cell; LOCAL_W]; LOCAL_H], rng: &mut SmallRng) {
    // Central open square with buildings around perimeter
    // Floor everything first
    for row in cells.iter_mut() { row.fill(Cell::Floor); }

    // Outer wall
    for x in 0..LOCAL_W { cells[0][x] = Cell::Wall; cells[LOCAL_H-1][x] = Cell::Wall; }
    for y in 0..LOCAL_H { cells[y][0] = Cell::Wall; cells[y][LOCAL_W-1] = Cell::Wall; }

    // Place 3-5 rectangular rooms as buildings
    let n_buildings = rng.gen_range(3..=5usize);
    for _ in 0..n_buildings {
        let bx = rng.gen_range(2..LOCAL_W - 6);
        let by = rng.gen_range(2..LOCAL_H - 6);
        let bw = rng.gen_range(3..=5usize);
        let bh = rng.gen_range(3..=4usize);
        for ry in by..=(by+bh).min(LOCAL_H-1) {
            for rx in bx..=(bx+bw).min(LOCAL_W-1) {
                cells[ry][rx] = Cell::Wall;
            }
        }
        // Carve a door
        let door_side = rng.gen_range(0..4u8);
        match door_side {
            0 if by > 0           => cells[by][bx + bw/2] = Cell::Door,
            1 if by+bh < LOCAL_H-1 => cells[by+bh][bx + bw/2] = Cell::Door,
            2 if bx > 0           => cells[by + bh/2][bx] = Cell::Door,
            _ if bx+bw < LOCAL_W-1 => cells[by + bh/2][bx+bw] = Cell::Door,
            _ => {}
        }
    }

    // Scatter pillars in the square
    for _ in 0..4 {
        let px = rng.gen_range(4..LOCAL_W - 4);
        let py = rng.gen_range(4..LOCAL_H - 4);
        if cells[py][px] == Cell::Floor { cells[py][px] = Cell::Pillar; }
    }
}

fn gen_open(cells: &mut [[Cell; LOCAL_W]; LOCAL_H], rng: &mut SmallRng) {
    // Mostly open with sparse walls — roads, plains
    for row in cells.iter_mut() { row.fill(Cell::Floor); }
    for x in 0..LOCAL_W { cells[0][x] = Cell::Wall; cells[LOCAL_H-1][x] = Cell::Wall; }
    for y in 0..LOCAL_H { cells[y][0] = Cell::Wall; cells[y][LOCAL_W-1] = Cell::Wall; }

    // A few scattered walls
    for _ in 0..rng.gen_range(3..8usize) {
        let wx = rng.gen_range(2..LOCAL_W - 2);
        let wy = rng.gen_range(2..LOCAL_H - 2);
        let wl = rng.gen_range(2..5usize);
        for i in 0..wl {
            if wx + i < LOCAL_W - 1 { cells[wy][wx+i] = Cell::Wall; }
        }
    }
}

fn gen_forest(cells: &mut [[Cell; LOCAL_W]; LOCAL_H], rng: &mut SmallRng) {
    // Dense walls (trees) with winding paths
    for row in cells.iter_mut() { row.fill(Cell::Wall); }

    // Carve winding corridors
    let mut cx = LOCAL_W / 2;
    let mut cy = LOCAL_H / 2;
    for _ in 0..200 {
        cells[cy][cx] = Cell::Floor;
        match rng.gen_range(0..4u8) {
            0 if cy > 1           => cy -= 1,
            1 if cy < LOCAL_H - 2 => cy += 1,
            2 if cx > 1           => cx -= 1,
            _                     => { if cx < LOCAL_W - 2 { cx += 1 } }
        }
    }

    // Water pools
    for _ in 0..2 {
        let wx = rng.gen_range(2..LOCAL_W - 3);
        let wy = rng.gen_range(2..LOCAL_H - 3);
        cells[wy][wx] = Cell::Water;
        cells[wy][wx+1] = Cell::Water;
        cells[wy+1][wx] = Cell::Water;
    }
}

fn gen_ruins(cells: &mut [[Cell; LOCAL_W]; LOCAL_H], rng: &mut SmallRng) {
    // Partially collapsed structures with rubble
    for row in cells.iter_mut() { row.fill(Cell::Floor); }
    for x in 0..LOCAL_W { cells[0][x] = Cell::Wall; cells[LOCAL_H-1][x] = Cell::Wall; }
    for y in 0..LOCAL_H { cells[y][0] = Cell::Wall; cells[y][LOCAL_W-1] = Cell::Wall; }

    // Ruined walls — broken lines
    for _ in 0..rng.gen_range(5..12usize) {
        let wx = rng.gen_range(1..LOCAL_W - 3);
        let wy = rng.gen_range(1..LOCAL_H - 3);
        let wl = rng.gen_range(2..7usize);
        let vertical = rng.gen_bool(0.5);
        for i in 0..wl {
            if rng.gen_bool(0.7) { // gaps in walls = ruin effect
                if vertical && wy+i < LOCAL_H-1 { cells[wy+i][wx] = Cell::Wall; }
                else if wx+i < LOCAL_W-1        { cells[wy][wx+i] = Cell::Wall; }
            } else {
                if vertical && wy+i < LOCAL_H-1 { cells[wy+i][wx] = Cell::Rubble; }
                else if wx+i < LOCAL_W-1        { cells[wy][wx+i] = Cell::Rubble; }
            }
        }
    }
}

fn gen_desert(cells: &mut [[Cell; LOCAL_W]; LOCAL_H], rng: &mut SmallRng) {
    for row in cells.iter_mut() { row.fill(Cell::Floor); }
    for x in 0..LOCAL_W { cells[0][x] = Cell::Wall; cells[LOCAL_H-1][x] = Cell::Wall; }
    for y in 0..LOCAL_H { cells[y][0] = Cell::Wall; cells[y][LOCAL_W-1] = Cell::Wall; }

    // Rock formations — clusters of walls
    for _ in 0..rng.gen_range(4..8usize) {
        let cx = rng.gen_range(2..LOCAL_W - 2);
        let cy = rng.gen_range(2..LOCAL_H - 2);
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let nx = (cx as i32 + dx) as usize;
                let ny = (cy as i32 + dy) as usize;
                if rng.gen_bool(0.6) && nx < LOCAL_W && ny < LOCAL_H {
                    cells[ny][nx] = Cell::Wall;
                }
            }
        }
    }

    // Oasis water
    if rng.gen_bool(0.5) {
        let wx = rng.gen_range(3..LOCAL_W - 3);
        let wy = rng.gen_range(3..LOCAL_H - 3);
        cells[wy][wx] = Cell::Water;
    }
}

fn npc_glyph(role: &str) -> char {
    match role {
        "lord"        => '☩',
        "merchant"    => '₪',
        "soldier" | "warlord" => '♂',
        "assassin"    => '†',
        "healer"      => '✚',
        "scholar"     => '¶',
        "bard"        => '♪',
        "priest"      => '✝',
        "thief"       => '‼',
        _             => '☺',
    }
}

fn item_name(kind: ItemKind, rng: &mut SmallRng) -> &'static str {
    match kind {
        ItemKind::Food   => *["dried meat", "hard bread", "salted fish", "root vegetables"].iter().nth(rng.gen_range(0..4)).unwrap(),
        ItemKind::Weapon => *["shortsword", "hand axe", "iron dagger", "worn spear"].iter().nth(rng.gen_range(0..4)).unwrap(),
        ItemKind::Armor  => *["leather vest", "chain coif", "iron buckler", "padded gambeson"].iter().nth(rng.gen_range(0..4)).unwrap(),
        ItemKind::Gold   => *["silver coin", "copper pennies", "gold ring", "trade token"].iter().nth(rng.gen_range(0..4)).unwrap(),
        ItemKind::Scroll => *["burned letter", "torn map", "wax-sealed notice", "census fragment"].iter().nth(rng.gen_range(0..4)).unwrap(),
    }
}
