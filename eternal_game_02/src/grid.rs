//! The play-field grid: the single source of truth for the game world.
//!
//! The world is a small 2D array of [`Cell`]s. Rendering is only a *view* of
//! this array: we keep an [`Image`](bevy::image::Image) the same size as the
//! grid and blit it onto a stretched sprite.
//!
//! Cells are only `0 Empty` or `1 Solid` (the pen). There is no stored surface:
//! the ball walks clockwise around a solid anchor and only ever steps onto a
//! cell 8-adjacent to a solid, so "track" is simply "not solid". See
//! [`Grid::is_open`].

use bevy::prelude::*;
use std::collections::HashSet;

/// Grid width in cells.
pub const GRID_W: i32 = 160;
/// Grid height in cells.
pub const GRID_H: i32 = 120;
/// How many world units one cell covers.
pub const CELL_PX: f32 = 6.0;
/// How many texture pixels one cell covers in the grid image, so a cell can
/// carry a detailed tile (the metal block). Must match `solid_block.png`.
pub const CELL_TEX: i32 = 16;

/// The eight neighbours, in counter-clockwise order starting east.
///
/// Used for "is this cell next to a solid" and for the ball's anchor ring.
pub const NEIGHBORS8: [IVec2; 8] = [
    IVec2::new(1, 0),
    IVec2::new(1, 1),
    IVec2::new(0, 1),
    IVec2::new(-1, 1),
    IVec2::new(-1, 0),
    IVec2::new(-1, -1),
    IVec2::new(0, -1),
    IVec2::new(1, -1),
];

/// The four orthogonal neighbours. Ball movement is orthogonal only; a diagonal
/// is represented as a combo of two of these.
pub const NEIGHBORS4: [IVec2; 4] = [
    IVec2::new(1, 0),
    IVec2::new(0, 1),
    IVec2::new(-1, 0),
    IVec2::new(0, -1),
];

/// What occupies a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Cell {
    /// `0` — nothing here.
    #[default]
    Empty,
    /// `1` — the white mass the pen draws.
    Solid,
}

/// The game world as a flat array of cells, plus the texture that displays it.
#[derive(Resource)]
pub struct Grid {
    pub w: i32,
    pub h: i32,
    pub cells: Vec<Cell>,
    /// Set whenever `cells` changes, so `sync_image` knows to re-upload.
    pub dirty: bool,
    /// The image we blit the grid into.
    pub image: Handle<Image>,
    /// Cells that belong to the level; the eraser may not remove these solids.
    pub locked: HashSet<IVec2>,
    /// Victory dissolve progress: `0.0` intact, `1.0` fully ashed and gone.
    pub dissolve: f32,
    /// The cell the dissolve wave spreads out from (the ball).
    pub dissolve_origin: IVec2,
    /// How far, in cells, the dissolve wave reaches, used to normalise it.
    pub dissolve_radius: f32,
}

impl Grid {
    pub fn new(image: Handle<Image>) -> Self {
        Self {
            w: GRID_W,
            h: GRID_H,
            cells: vec![Cell::Empty; (GRID_W * GRID_H) as usize],
            dirty: true,
            image,
            locked: HashSet::new(),
            dissolve: 0.0,
            dissolve_origin: IVec2::ZERO,
            dissolve_radius: 1.0,
        }
    }

    /// Lock the solid cells (the level's walls) so the eraser cannot remove
    /// them.
    pub fn lock_solids(&mut self) {
        self.locked.clear();
        for y in 0..self.h {
            for x in 0..self.w {
                let cell = IVec2::new(x, y);
                if self.get(cell) == Some(Cell::Solid) {
                    self.locked.insert(cell);
                }
            }
        }
    }

    /// Shift every solid so its bounding box is centred on the board, and return
    /// the shift applied (so a placed start can follow it).
    ///
    /// This lets a level be drawn anywhere in the editor and still appear
    /// centred when it is loaded. The board's centre sits between cells
    /// `(w-1)/2` and `w/2`, so doubled coordinates are used to avoid a
    /// rounding bias.
    pub fn recenter_solids(&mut self) -> IVec2 {
        let mut min = IVec2::splat(i32::MAX);
        let mut max = IVec2::splat(i32::MIN);
        let mut any = false;
        for y in 0..self.h {
            for x in 0..self.w {
                let cell = IVec2::new(x, y);
                if self.get(cell) == Some(Cell::Solid) {
                    min = min.min(cell);
                    max = max.max(cell);
                    any = true;
                }
            }
        }
        if !any {
            return IVec2::ZERO;
        }
        let bbox_centre2 = min + max;
        let board_centre2 = IVec2::new(self.w - 1, self.h - 1);
        let shift = IVec2::new(
            ((board_centre2.x - bbox_centre2.x) as f32 / 2.0).round() as i32,
            ((board_centre2.y - bbox_centre2.y) as f32 / 2.0).round() as i32,
        );
        if shift != IVec2::ZERO {
            let old = std::mem::take(&mut self.cells);
            let mut cells = vec![Cell::Empty; old.len()];
            for (i, cell) in old.iter().enumerate() {
                if *cell == Cell::Solid {
                    let src = IVec2::new(i as i32 % self.w, i as i32 / self.w);
                    if let Some(dst) = self.index(src + shift) {
                        cells[dst] = Cell::Solid;
                    }
                }
            }
            self.cells = cells;
            self.dirty = true;
        }
        shift
    }

    fn index(&self, cell: IVec2) -> Option<usize> {
        (cell.x >= 0 && cell.y >= 0 && cell.x < self.w && cell.y < self.h)
            .then(|| (cell.y * self.w + cell.x) as usize)
    }

    pub fn get(&self, cell: IVec2) -> Option<Cell> {
        self.index(cell).map(|i| self.cells[i])
    }

    /// Raw write. Prefer [`Grid::paint`] when drawing.
    pub fn set(&mut self, cell: IVec2, value: Cell) {
        if let Some(i) = self.index(cell)
            && self.cells[i] != value
        {
            self.cells[i] = value;
            self.dirty = true;
        }
    }

    /// Is this cell solid?
    pub fn is_solid(&self, cell: IVec2) -> bool {
        self.get(cell) == Some(Cell::Solid)
    }

    /// Is this cell somewhere the ball may stand — empty and next to a solid?
    ///
    /// With the anchor model, every step lands on a cell 8-adjacent to a solid,
    /// so this is exactly the tracked surface. It needs no stored state.
    pub fn is_open(&self, cell: IVec2) -> bool {
        self.get(cell) == Some(Cell::Empty)
            && NEIGHBORS8
                .iter()
                .any(|offset| self.is_solid(cell + *offset))
    }

    /// The pen. `Cell::Solid` draws a wall; `Cell::Empty` erases it. The eraser
    /// must not remove level solids, but the pen may still add.
    pub fn paint(&mut self, cell: IVec2, value: Cell) {
        if value == Cell::Empty && self.locked.contains(&cell) {
            return;
        }
        self.set(cell, value);
    }

    /// Paint a straight line of cells with `value` (Bresenham), so fast drags
    /// don't leave gaps.
    pub fn paint_line(&mut self, a: IVec2, b: IVec2, value: Cell) {
        let (mut x, mut y) = (a.x, a.y);
        let (dx, dy) = ((b.x - a.x).abs(), -(b.y - a.y).abs());
        let (sx, sy) = (
            if a.x < b.x { 1 } else { -1 },
            if a.y < b.y { 1 } else { -1 },
        );
        let mut err = dx + dy;
        loop {
            self.paint(IVec2::new(x, y), value);
            if x == b.x && y == b.y {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Wipe everything except locked solids.
    pub fn clear(&mut self) {
        for y in 0..self.h {
            for x in 0..self.w {
                let cell = IVec2::new(x, y);
                if !self.locked.contains(&cell) {
                    self.set(cell, Cell::Empty);
                }
            }
        }
    }

    /// Wipe the whole board — solids and the level lock. Used when the victory
    /// detonation consumes the level.
    pub fn obliterate(&mut self) {
        self.locked.clear();
        self.cells.iter_mut().for_each(|c| *c = Cell::Empty);
        self.dissolve = 0.0;
        self.dirty = true;
    }

    /// Bounding box of every solid cell, or `None` if the board is blank.
    /// Doubles as the level's extent for the victory blast.
    pub fn content_bounds(&self) -> Option<(IVec2, IVec2)> {
        let mut min = IVec2::new(self.w, self.h);
        let mut max = IVec2::new(-1, -1);
        for y in 0..self.h {
            for x in 0..self.w {
                if self.get(IVec2::new(x, y)) != Some(Cell::Empty) {
                    min.x = min.x.min(x);
                    min.y = min.y.min(y);
                    max.x = max.x.max(x);
                    max.y = max.y.max(y);
                }
            }
        }
        (max.x >= 0).then_some((min, max))
    }

    /// Label every solid cell with an 8-connected component id. The ball uses
    /// the labels to pick its first anchor.
    pub fn solid_components(&self) -> std::collections::HashMap<IVec2, usize> {
        let mut labels: std::collections::HashMap<IVec2, usize> = std::collections::HashMap::new();
        let mut next_id = 0;
        for y in 0..self.h {
            for x in 0..self.w {
                let seed = IVec2::new(x, y);
                if self.get(seed) != Some(Cell::Solid) || labels.contains_key(&seed) {
                    continue;
                }
                labels.insert(seed, next_id);
                let mut stack = vec![seed];
                while let Some(cell) = stack.pop() {
                    for d in NEIGHBORS8 {
                        let n = cell + d;
                        if self.get(n) == Some(Cell::Solid) && !labels.contains_key(&n) {
                            labels.insert(n, next_id);
                            stack.push(n);
                        }
                    }
                }
                next_id += 1;
            }
        }
        labels
    }

    /// The topmost open cell (tie-break: leftmost) — the default start.
    pub fn find_start(&self) -> Option<IVec2> {
        let mut best: Option<IVec2> = None;
        for y in 0..self.h {
            for x in 0..self.w {
                let c = IVec2::new(x, y);
                if self.is_open(c) {
                    best = Some(match best {
                        None => c,
                        Some(b) if c.y > b.y || (c.y == b.y && c.x < b.x) => c,
                        Some(b) => b,
                    });
                }
            }
        }
        best
    }

    /// True if any solid is not part of the locked level — i.e. the player has
    /// drawn something of their own. Used by the tutorial.
    pub fn has_unlocked_solid(&self) -> bool {
        for y in 0..self.h {
            for x in 0..self.w {
                let cell = IVec2::new(x, y);
                if self.get(cell) == Some(Cell::Solid) && !self.locked.contains(&cell) {
                    return true;
                }
            }
        }
        false
    }

    /// World position -> grid cell, if the position is inside the grid.
    ///
    /// The grid is centred on world `(0, 0)`; `+y` is up, matching Bevy.
    pub fn world_to_cell(&self, world: Vec2) -> Option<IVec2> {
        let fx = world.x / CELL_PX + self.w as f32 / 2.0;
        let fy = world.y / CELL_PX + self.h as f32 / 2.0;
        if fx < 0.0 || fy < 0.0 || fx >= self.w as f32 || fy >= self.h as f32 {
            return None;
        }
        Some(IVec2::new(fx as i32, fy as i32))
    }

    /// Grid cell -> centre of that cell in world space.
    pub fn cell_to_world(&self, cell: IVec2) -> Vec2 {
        Vec2::new(
            (cell.x as f32 + 0.5 - self.w as f32 / 2.0) * CELL_PX,
            (cell.y as f32 + 0.5 - self.h as f32 / 2.0) * CELL_PX,
        )
    }

    /// Total world size of the grid.
    pub fn world_size(&self) -> Vec2 {
        Vec2::new(self.w as f32, self.h as f32) * CELL_PX
    }

    /// Pixel colour for a cell, as raw sRGB bytes ready for the texture.
    pub fn color(cell: Cell) -> [u8; 4] {
        match cell {
            Cell::Empty => [0, 0, 0, 0],
            Cell::Solid => [104, 112, 130, 255],
        }
    }

    /// A cell's colour while the victory dissolve passes over it. `progress`
    /// runs from `0.0` (intact) to `1.0` (burnt away): first to warm ash, then
    /// to the background, so the level visibly crumbles rather than blinking
    /// out.
    pub fn dissolve_color(cell: Cell, progress: f32) -> [u8; 4] {
        // Empty cells are just background; only the level burns.
        if progress <= 0.0 || cell == Cell::Empty {
            return Self::color(cell);
        }
        let ash = [72, 66, 60, 255];
        let gone = Self::color(Cell::Empty);
        if progress < 0.5 {
            lerp_rgba(Self::color(cell), ash, progress * 2.0)
        } else {
            lerp_rgba(ash, gone, (progress - 0.5) * 2.0)
        }
    }

    /// Number of cells currently holding `value` (tests).
    #[cfg(test)]
    pub fn count(&self, value: Cell) -> usize {
        self.cells.iter().filter(|c| **c == value).count()
    }
}

/// Linear blend between two RGBA byte colours.
fn lerp_rgba(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let mut out = [0u8; 4];
    for i in 0..4 {
        out[i] = (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        Grid::new(Handle::default())
    }

    fn paint(grid: &mut Grid, cell: IVec2, value: Cell) {
        grid.paint(cell, value);
    }

    /// A single solid opens its eight neighbours as track.
    #[test]
    fn a_solid_opens_its_eight_neighbours() {
        let mut grid = grid();
        paint(&mut grid, IVec2::new(5, 5), Cell::Solid);
        assert!(grid.is_solid(IVec2::new(5, 5)));
        for offset in NEIGHBORS8 {
            assert!(
                grid.is_open(IVec2::new(5, 5) + offset),
                "neighbour {offset:?} should be open"
            );
        }
        // A cell two away, and the solid itself, are not open.
        assert!(!grid.is_open(IVec2::new(7, 5)));
        assert!(!grid.is_open(IVec2::new(5, 5)));
    }

    /// Erasing the only solid closes its neighbours again.
    #[test]
    fn erasing_closes_the_neighbours() {
        let mut grid = grid();
        paint(&mut grid, IVec2::new(5, 5), Cell::Solid);
        paint(&mut grid, IVec2::new(5, 5), Cell::Empty);
        assert!(!grid.is_open(IVec2::new(6, 5)));
    }

    /// `find_start` returns an open cell next to the drawn solid.
    #[test]
    fn find_start_lands_next_to_a_solid() {
        let mut grid = grid();
        paint(&mut grid, IVec2::new(5, 5), Cell::Solid);
        let start = grid.find_start().unwrap();
        assert!(grid.is_open(start));
        assert!(
            NEIGHBORS8
                .iter()
                .any(|d| grid.is_solid(start + *d))
        );
    }

    /// A level drawn off-centre is recentred on the board by the loader.
    #[test]
    fn recenter_solids_centres_the_bounding_box() {
        let mut grid = grid();
        for y in 10..14 {
            for x in 10..14 {
                grid.set(IVec2::new(x, y), Cell::Solid);
            }
        }

        let shift = grid.recenter_solids();

        // A 4x4 block drawn at 10..13 recentres exactly on a 160x120 board.
        assert_eq!(shift, IVec2::new(68, 48));
        let (min, max) = grid.content_bounds().unwrap();
        assert_eq!(min, IVec2::new(78, 58));
        assert_eq!(max, IVec2::new(81, 61));
        assert_eq!(grid.count(Cell::Solid), 16);
    }

    /// The victory dissolve burns a cell from its colour to ash, then away.
    #[test]
    fn dissolve_color_burns_to_ash_then_gone() {
        let base = Grid::color(Cell::Solid);
        assert_eq!(Grid::dissolve_color(Cell::Solid, 0.0), base);
        // The empty background never ashes over.
        assert_eq!(
            Grid::dissolve_color(Cell::Empty, 0.5),
            Grid::color(Cell::Empty)
        );

        let ash = Grid::dissolve_color(Cell::Solid, 0.5);
        assert_ne!(ash, base);
        // Ash is darker than the pale solid.
        assert!(ash[0] < base[0] && ash[1] < base[1] && ash[2] < base[2]);

        // Burnt all the way down to the background.
        assert_eq!(
            Grid::dissolve_color(Cell::Solid, 1.0),
            Grid::color(Cell::Empty)
        );
    }

    /// The detonation wipes the whole board, including the lock.
    #[test]
    fn obliterate_clears_everything() {
        let mut grid = grid();
        paint(&mut grid, IVec2::new(5, 5), Cell::Solid);
        grid.lock_solids();
        assert!(grid.count(Cell::Solid) > 0);

        grid.obliterate();

        assert_eq!(grid.count(Cell::Empty), (GRID_W * GRID_H) as usize);
        assert!(grid.locked.is_empty());
        assert!(grid.dirty);
    }

    /// The content bounds are the solid bounding box.
    #[test]
    fn content_bounds_covers_the_drawn_cells() {
        let mut grid = grid();
        paint(&mut grid, IVec2::new(5, 5), Cell::Solid);
        paint(&mut grid, IVec2::new(8, 9), Cell::Solid);
        let (min, max) = grid.content_bounds().expect("non-empty");
        assert_eq!(min, IVec2::new(5, 5));
        assert_eq!(max, IVec2::new(8, 9));
    }

    /// Player-drawn solids count as "drawn"; locked level solids do not.
    #[test]
    fn only_unlocked_solids_count_as_drawn() {
        let mut grid = grid();
        paint(&mut grid, IVec2::new(5, 5), Cell::Solid);
        grid.lock_solids();
        assert!(!grid.has_unlocked_solid());

        paint(&mut grid, IVec2::new(10, 10), Cell::Solid);
        assert!(grid.has_unlocked_solid());
    }

    /// The eraser can't remove level solids, but the player's own solids are
    /// fair game.
    #[test]
    fn locked_solids_cannot_be_erased() {
        let mut grid = grid();
        paint(&mut grid, IVec2::new(5, 5), Cell::Solid);
        grid.lock_solids();

        // Player draws their own solid elsewhere.
        paint(&mut grid, IVec2::new(10, 10), Cell::Solid);

        // Level solid is protected.
        paint(&mut grid, IVec2::new(5, 5), Cell::Empty);
        assert_eq!(grid.get(IVec2::new(5, 5)), Some(Cell::Solid));
        // Player's own solid is not.
        paint(&mut grid, IVec2::new(10, 10), Cell::Empty);
        assert_eq!(grid.get(IVec2::new(10, 10)), Some(Cell::Empty));
    }
}
