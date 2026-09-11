//! The play-field grid: the single source of truth for the game world.
//!
//! The world is a small 2D array of [`Cell`]s. Rendering is only a *view* of
//! this array: we keep an [`Image`](bevy::image::Image) the same size as the
//! grid and blit it onto a stretched sprite.
//!
//! Cell values follow the design doc:
//! `0 Empty`, `1 Solid` (pen), `2 Surface` (auto-generated track), `3 Trail`.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

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
/// Used both for "grow surface around solids" and for the ball's movement.
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
    /// `2` — track the ball can travel on, grown around solids.
    Surface,
    /// `3` — a surface cell the ball has already covered.
    Trail,
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
    /// The authoritative set of solid cells. The surface is regenerated from
    /// this whenever `solids_dirty` is set.
    pub solids: HashSet<IVec2>,
    /// Set when `solids` changes, so the surface can be regenerated.
    pub solids_dirty: bool,
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
            solids: HashSet::new(),
            solids_dirty: false,
            dissolve: 0.0,
            dissolve_origin: IVec2::ZERO,
            dissolve_radius: 1.0,
        }
    }

    /// Lock the solid cells (the level's walls) so the eraser cannot remove
    /// them. The derived surface is *not* locked.
    pub fn lock_solids(&mut self) {
        self.locked.clear();
        self.solids.clear();
        for y in 0..self.h {
            for x in 0..self.w {
                let cell = IVec2::new(x, y);
                if self.get(cell) == Some(Cell::Solid) {
                    self.locked.insert(cell);
                    self.solids.insert(cell);
                }
            }
        }
    }

    /// Shift every solid so the solid bounding box is centred on the board,
    /// and return the shift applied (so a placed start can follow it).
    ///
    /// This lets a level be drawn anywhere in the editor and still appear
    /// centred when it is loaded. The board's centre sits between cells
    /// `(w-1)/2` and `w/2`, so doubled coordinates are used to avoid a
    /// rounding bias.
    pub fn recenter_solids(&mut self) -> IVec2 {
        if self.solids.is_empty() {
            return IVec2::ZERO;
        }
        let mut min = IVec2::splat(i32::MAX);
        let mut max = IVec2::splat(i32::MIN);
        for solid in &self.solids {
            min = min.min(*solid);
            max = max.max(*solid);
        }
        let bbox_centre2 = min + max;
        let board_centre2 = IVec2::new(self.w - 1, self.h - 1);
        let shift = IVec2::new(
            ((board_centre2.x - bbox_centre2.x) as f32 / 2.0).round() as i32,
            ((board_centre2.y - bbox_centre2.y) as f32 / 2.0).round() as i32,
        );
        if shift != IVec2::ZERO {
            self.solids = self.solids.iter().map(|solid| *solid + shift).collect();
            self.solids_dirty = true;
            self.regenerate_surfaces();
        }
        shift
    }

    /// Rebuild every derived surface cell from `solids`. Cheap enough to run
    /// whenever the matrix is dirty.
    pub fn regenerate_surfaces(&mut self) {
        if !self.solids_dirty {
            return;
        }
        self.solids_dirty = false;
        self.cells.iter_mut().for_each(|c| *c = Cell::Empty);
        let solids: Vec<IVec2> = self.solids.iter().copied().collect();
        for solid in solids {
            self.set(solid, Cell::Solid);
            for n in self.neighbors(solid).collect::<Vec<_>>() {
                if self.get(n) == Some(Cell::Empty) {
                    self.set(n, Cell::Surface);
                }
            }
        }
        self.dirty = true;
    }

    fn index(&self, cell: IVec2) -> Option<usize> {
        (cell.x >= 0 && cell.y >= 0 && cell.x < self.w && cell.y < self.h)
            .then(|| (cell.y * self.w + cell.x) as usize)
    }

    pub fn get(&self, cell: IVec2) -> Option<Cell> {
        self.index(cell).map(|i| self.cells[i])
    }

    /// Raw write. Prefer [`Grid::paint`] when drawing, so surface stays in sync.
    pub fn set(&mut self, cell: IVec2, value: Cell) {
        if let Some(i) = self.index(cell)
            && self.cells[i] != value
        {
            self.cells[i] = value;
            self.dirty = true;
        }
    }

    fn neighbors(&self, cell: IVec2) -> impl Iterator<Item = IVec2> + '_ {
        NEIGHBORS8.iter().map(move |d| cell + *d)
    }

    /// Is this cell part of the track the ball may stand on?
    pub fn is_track(&self, cell: IVec2) -> bool {
        matches!(self.get(cell), Some(Cell::Surface) | Some(Cell::Trail))
    }

    /// The pen. `Cell::Solid` lays down a `1` and grows `2` surface on every
    /// adjacent empty cell; `Cell::Empty` erases and cleans up surface that no
    /// longer touches any solid.
    pub fn paint(&mut self, cell: IVec2, value: Cell) {
        // The eraser must not remove level solids, but the pen may still add.
        if value == Cell::Empty && self.locked.contains(&cell) {
            return;
        }
        let changed = match value {
            Cell::Solid => self.solids.insert(cell),
            Cell::Empty => self.solids.remove(&cell),
            _ => false,
        };
        if changed {
            self.solids_dirty = true;
        }
    }

    /// Regenerate the whole `2` surface from the `1` solids, discarding trail.
    ///
    /// The surface is derived data, so a config only really needs to store the
    /// solids; this rebuilds everything else.
    pub fn rebuild_surface(&mut self) {
        self.solids = (0..self.h)
            .flat_map(|y| (0..self.w).map(move |x| IVec2::new(x, y)))
            .filter(|c| self.get(*c) == Some(Cell::Solid))
            .collect();
        for c in &mut self.cells {
            if *c != Cell::Solid {
                *c = Cell::Empty;
            }
        }
        let solids: Vec<IVec2> = (0..self.h)
            .flat_map(|y| (0..self.w).map(move |x| IVec2::new(x, y)))
            .filter(|c| self.get(*c) == Some(Cell::Solid))
            .collect();
        for solid in solids {
            for n in self.neighbors(solid).collect::<Vec<_>>() {
                if self.get(n) == Some(Cell::Empty) {
                    self.set(n, Cell::Surface);
                }
            }
        }
        self.dirty = true;
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

    /// Wipe everything except locked solids, then regrow their surface.
    pub fn clear(&mut self) {
        self.solids.retain(|cell| self.locked.contains(cell));
        self.solids_dirty = true;
        self.regenerate_surfaces();
    }

    /// Turn every visited cell back into fresh surface.
    pub fn reset_trail(&mut self) {
        for c in &mut self.cells {
            if *c == Cell::Trail {
                *c = Cell::Surface;
            }
        }
        self.dirty = true;
    }

    /// Wipe the whole board — solids, surface, trail and the level lock. Used
    /// when the victory detonation consumes the level.
    pub fn obliterate(&mut self) {
        self.solids.clear();
        self.locked.clear();
        self.cells.iter_mut().for_each(|c| *c = Cell::Empty);
        self.dissolve = 0.0;
        self.dirty = true;
    }

    /// Bounding box of every non-empty cell, or `None` if the board is blank.
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

    /// Flood fill the orthogonal track reachable from `start`. This is the
    /// ball's route.
    pub fn reachable(&self, start: IVec2) -> HashSet<IVec2> {
        let mut seen = HashSet::new();
        if !self.is_track(start) {
            return seen;
        }
        seen.insert(start);
        let mut stack = vec![start];
        while let Some(cell) = stack.pop() {
            for d in NEIGHBORS4 {
                let next = cell + d;
                if seen.contains(&next) || !self.is_track(next) {
                    continue;
                }
                seen.insert(next);
                stack.push(next);
            }
        }
        seen
    }

    /// Label every solid cell with an 8-connected component id. The ball uses
    /// this to stay on one connected mass instead of hopping between them.
    pub fn solid_components(&self) -> HashMap<IVec2, usize> {
        let mut labels: HashMap<IVec2, usize> = HashMap::new();
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

    /// The topmost surface cell (tie-break: leftmost) — the default start.
    pub fn find_start(&self) -> Option<IVec2> {
        let mut best: Option<IVec2> = None;
        for y in 0..self.h {
            for x in 0..self.w {
                if self.cells[(y * self.w + x) as usize] == Cell::Surface {
                    let c = IVec2::new(x, y);
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
            Cell::Surface => [58, 92, 150, 255],
            Cell::Trail => [70, 200, 150, 255],
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

    /// Number of cells currently holding `value` (tests / win checks).
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

    /// Paint a cell and rebuild the derived surface, as the app does.
    fn paint(grid: &mut Grid, cell: IVec2, value: Cell) {
        grid.paint(cell, value);
        grid.regenerate_surfaces();
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
        grid.rebuild_surface();

        let shift = grid.recenter_solids();

        // A 4x4 block drawn at 10..13 recentres exactly on a 160x120 board.
        assert_eq!(shift, IVec2::new(68, 48));
        let xs = grid.solids.iter().map(|c| c.x);
        let ys = grid.solids.iter().map(|c| c.y);
        assert_eq!((xs.clone().min(), xs.max()), (Some(78), Some(81)));
        assert_eq!((ys.clone().min(), ys.max()), (Some(58), Some(61)));
        // Surface was regenerated around the new position.
        assert!(grid.count(Cell::Surface) > 0);
    }

    /// The victory dissolve burns a cell from its colour to ash, then away.
    #[test]
    fn dissolve_color_burns_to_ash_then_gone() {
        let base = Grid::color(Cell::Solid);
        assert_eq!(Grid::dissolve_color(Cell::Solid, 0.0), base);
        // The empty background never ashes over.
        assert_eq!(Grid::dissolve_color(Cell::Empty, 0.5), Grid::color(Cell::Empty));

        let ash = Grid::dissolve_color(Cell::Solid, 0.5);
        assert_ne!(ash, base);
        // Ash is darker than the pale solid.
        assert!(ash[0] < base[0] && ash[1] < base[1] && ash[2] < base[2]);

        // Burnt all the way down to the background.
        assert_eq!(Grid::dissolve_color(Cell::Solid, 1.0), Grid::color(Cell::Empty));
    }

    /// The detonation wipes the whole board — solids, surface, trail and lock.
    #[test]
    fn obliterate_clears_everything() {
        let mut grid = grid();
        paint(&mut grid, IVec2::new(5, 5), Cell::Solid);
        grid.lock_solids();
        assert!(grid.count(Cell::Solid) > 0);

        grid.obliterate();

        assert_eq!(grid.count(Cell::Empty), (GRID_W * GRID_H) as usize);
        assert!(grid.solids.is_empty());
        assert!(grid.locked.is_empty());
        assert!(grid.dirty);
    }

    /// The content bounds cover the drawn shape plus its grown surface.
    #[test]
    fn content_bounds_covers_the_drawn_cells() {
        let mut grid = grid();
        paint(&mut grid, IVec2::new(5, 5), Cell::Solid);
        paint(&mut grid, IVec2::new(8, 9), Cell::Solid);
        let (min, max) = grid.content_bounds().expect("non-empty");
        assert_eq!(min, IVec2::new(4, 4));
        assert_eq!(max, IVec2::new(9, 10));
    }

    #[test]
    fn painting_a_solid_grows_eight_surface_cells() {
        let mut grid = grid();
        paint(&mut grid, IVec2::new(5, 5), Cell::Solid);
        assert_eq!(grid.get(IVec2::new(5, 5)), Some(Cell::Solid));
        assert_eq!(grid.count(Cell::Surface), 8);
    }

    #[test]
    fn erasing_cleans_up_orphaned_surface() {
        let mut grid = grid();
        paint(&mut grid, IVec2::new(5, 5), Cell::Solid);
        paint(&mut grid, IVec2::new(5, 5), Cell::Empty);
        assert_eq!(grid.count(Cell::Surface), 0);
    }

    #[test]
    fn a_solid_block_gets_an_outline() {
        let mut grid = grid();
        for y in 5..8 {
            for x in 5..8 {
                paint(&mut grid, IVec2::new(x, y), Cell::Solid);
            }
        }
        // The 5x5 neighbourhood minus the 3x3 solid block.
        assert_eq!(grid.count(Cell::Surface), 25 - 9);
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

    /// Reachability is orthogonal-only now, so a diagonal pair is not connected.
    #[test]
    fn reachable_is_orthogonal() {
        let mut grid = grid();
        grid.set(IVec2::new(1, 0), Cell::Surface);
        grid.set(IVec2::new(0, 1), Cell::Surface);

        let route = grid.reachable(IVec2::new(1, 0));
        assert!(route.contains(&IVec2::new(1, 0)));
        assert!(!route.contains(&IVec2::new(0, 1)));
    }
}
