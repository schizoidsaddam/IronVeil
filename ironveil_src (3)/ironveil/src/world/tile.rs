#![allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileKind {
    Grass,
    Forest,
    Mountain,
    Water,
    Desert,
    Swamp,
    Town,
    Dungeon,
    Road,
    Ruin,
    Rift,   // swap-pressure planar anomaly
}

#[derive(Debug, Clone)]
pub struct Tile {
    pub kind:    TileKind,
    pub visible: bool,
    pub explored: bool,
    pub elevation: u8,
}

impl Tile {
    pub fn new(kind: TileKind) -> Self {
        Self {
            kind,
            visible:   false,
            explored:  false,
            elevation: 0,
        }
    }

    pub fn glyph(&self) -> char {
        if !self.explored {
            return ' ';
        }
        match self.kind {
            TileKind::Grass    => '.',
            TileKind::Forest   => '♣',
            TileKind::Mountain => '▲',
            TileKind::Water    => '~',
            TileKind::Desert   => '░',
            TileKind::Swamp    => '%',
            TileKind::Town     => '⌂',
            TileKind::Dungeon  => '▼',
            TileKind::Road     => '═',
            TileKind::Ruin     => '#',
            TileKind::Rift     => '?',
        }
    }

    pub fn passable(&self) -> bool {
        !matches!(self.kind, TileKind::Water | TileKind::Mountain)
    }
}
