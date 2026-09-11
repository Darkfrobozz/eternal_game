//! The play-field grid: the single source of truth for the game world.
//!
//! The world is a small 2D array of [`Cell`]s. Rendering is only a *view* of
//! this array: we keep an [`Image`](bevy::image::Image) the same size as the
//! grid and blit it onto a stretched sprite.
//!
//! Cell values follow the design doc:
//! `0 Empty`, `1 Solid` (pen), `2 Surface` (auto-generated track), `3 Trail`.

use bevy::prelude::*;
use std::collections::HashSet;

/// Grid width in cells.
pub const GRID_W: i32 = 160;
/// Grid height in cells.
pub const GRID_H: i32 = 120;
/// How many world units one cell covers.
pub const CELL_PX: f32 = 6.0;

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
}

impl Grid {
    pub fn new(image: Handle<Image>) -> Self {
        Self {
            w: GRID_W,
            h: GRID_H,
            cells: vec![Cell::Empty; (GRID_W * GRID_H) as usize],
            dirty: true,
            image,
        }
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

    fn has_solid_neighbor(&self, cell: IVec2) -> bool {
        self.neighbors(cell)
            .any(|n| self.get(n) == Some(Cell::Solid))
    }

    /// Is this cell part of the track the ball may stand on?
    pub fn is_track(&self, cell: IVec2) -> bool {
        matches!(self.get(cell), Some(Cell::Surface) | Some(Cell::Trail))
    }

    /// A solid, or off the edge of the world (edges act as walls).
    fn solid_or_out(&self, cell: IVec2) -> bool {
        match self.get(cell) {
            Some(Cell::Solid) | None => true,
            _ => false,
        }
    }

    /// True when a *diagonal* step would squeeze between two solids (or the
    /// grid edge), i.e. cut across a wall corner. Straight steps never block.
    pub fn step_blocked(&self, from: IVec2, dir: IVec2) -> bool {
        if dir.x == 0 || dir.y == 0 {
            return false;
        }
        self.solid_or_out(from + IVec2::new(dir.x, 0))
            && self.solid_or_out(from + IVec2::new(0, dir.y))
    }

    /// Solid cells adjacent to both `a` and `b` — the contour(s) they share.
    pub fn common_solids(&self, a: IVec2, b: IVec2) -> Vec<IVec2> {
        NEIGHBORS8
            .iter()
            .filter_map(|offset| {
                let solid = a + *offset;
                if self.get(solid) != Some(Cell::Solid) {
                    return None;
                }
                let d = solid - b;
                (d.x.abs() <= 1 && d.y.abs() <= 1).then_some(solid)
            })
            .collect()
    }

    /// True when `a` and `b` are both adjacent to some common solid cell.
    #[cfg(test)]
    pub fn shares_solid(&self, a: IVec2, b: IVec2) -> bool {
        !self.common_solids(a, b).is_empty()
    }

    /// The pen. `Cell::Solid` lays down a `1` and grows `2` surface on every
    /// adjacent empty cell; `Cell::Empty` erases and cleans up surface that no
    /// longer touches any solid.
    pub fn paint(&mut self, cell: IVec2, value: Cell) {
        self.set(cell, value);
        match value {
            Cell::Solid => {
                for n in self.neighbors(cell).collect::<Vec<_>>() {
                    if self.get(n) == Some(Cell::Empty) {
                        self.set(n, Cell::Surface);
                    }
                }
            }
            Cell::Empty => {
                for n in self.neighbors(cell).collect::<Vec<_>>() {
                    if self.get(n) == Some(Cell::Surface) && !self.has_solid_neighbor(n) {
                        self.set(n, Cell::Empty);
                    }
                }
            }
            _ => {}
        }
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

    /// Wipe the board.
    pub fn clear(&mut self) {
        self.cells.iter_mut().for_each(|c| *c = Cell::Empty);
        self.dirty = true;
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

    /// Flood fill the track reachable from `start`, respecting the no-corner-
    /// cutting rule. This is the ball's route: cells across a wall are in a
    /// different component and are never touched.
    pub fn reachable(&self, start: IVec2) -> HashSet<IVec2> {
        let mut seen = HashSet::new();
        if !self.is_track(start) {
            return seen;
        }
        seen.insert(start);
        let mut stack = vec![start];
        while let Some(cell) = stack.pop() {
            for d in NEIGHBORS8 {
                let next = cell + d;
                if seen.contains(&next)
                    || !self.is_track(next)
                    || self.step_blocked(cell, d)
                {
                    continue;
                }
                seen.insert(next);
                stack.push(next);
            }
        }
        seen
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
            Cell::Empty => [22, 22, 30, 255],
            Cell::Solid => [232, 235, 242, 255],
            Cell::Surface => [58, 92, 150, 255],
            Cell::Trail => [70, 200, 150, 255],
        }
    }

    /// Number of cells currently holding `value` (tests / win checks).
    #[cfg(test)]
    pub fn count(&self, value: Cell) -> usize {
        self.cells.iter().filter(|c| **c == value).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        Grid::new(Handle::default())
    }

    #[test]
    fn painting_a_solid_grows_eight_surface_cells() {
        let mut grid = grid();
        grid.paint(IVec2::new(5, 5), Cell::Solid);
        assert_eq!(grid.get(IVec2::new(5, 5)), Some(Cell::Solid));
        assert_eq!(grid.count(Cell::Surface), 8);
    }

    #[test]
    fn erasing_cleans_up_orphaned_surface() {
        let mut grid = grid();
        grid.paint(IVec2::new(5, 5), Cell::Solid);
        grid.paint(IVec2::new(5, 5), Cell::Empty);
        assert_eq!(grid.count(Cell::Surface), 0);
    }

    #[test]
    fn a_solid_block_gets_an_outline() {
        let mut grid = grid();
        for y in 5..8 {
            for x in 5..8 {
                grid.paint(IVec2::new(x, y), Cell::Solid);
            }
        }
        // The 5x5 neighbourhood minus the 3x3 solid block.
        assert_eq!(grid.count(Cell::Surface), 25 - 9);
    }

    /// Two solids meeting at a corner must not let the track leak through the
    /// diagonal gap between them.
    #[test]
    fn diagonal_wall_is_not_cut() {
        let mut grid = grid();
        grid.set(IVec2::new(0, 0), Cell::Solid);
        grid.set(IVec2::new(1, 1), Cell::Solid);
        grid.set(IVec2::new(1, 0), Cell::Surface);
        grid.set(IVec2::new(0, 1), Cell::Surface);

        assert!(grid.step_blocked(IVec2::new(1, 0), IVec2::new(-1, 1)));
        let route = grid.reachable(IVec2::new(1, 0));
        assert!(!route.contains(&IVec2::new(0, 1)));
    }

    /// ...but an open diagonal (no solids pinching the corner) still connects.
    #[test]
    fn open_diagonal_still_connects() {
        let mut grid = grid();
        grid.set(IVec2::new(1, 0), Cell::Surface);
        grid.set(IVec2::new(0, 1), Cell::Surface);

        let route = grid.reachable(IVec2::new(1, 0));
        assert!(route.contains(&IVec2::new(0, 1)));
    }

    /// Two facing walls are separate contours even where their surfaces touch.
    #[test]
    fn parallel_walls_do_not_share_a_contour() {
        let mut grid = grid();
        for y in 10..20 {
            grid.set(IVec2::new(0, y), Cell::Solid);
            grid.set(IVec2::new(3, y), Cell::Solid);
            grid.set(IVec2::new(1, y), Cell::Surface);
            grid.set(IVec2::new(2, y), Cell::Surface);
        }
        // Sideways hop between the two walls' surfaces is not the same contour.
        assert!(!grid.shares_solid(IVec2::new(1, 15), IVec2::new(2, 15)));
        // But continuing along one wall is.
        assert!(grid.shares_solid(IVec2::new(1, 15), IVec2::new(1, 16)));
    }
}
