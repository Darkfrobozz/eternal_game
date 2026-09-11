//! Phase 2: the ball that walks the generated surface.
//!
//! The ball is a plain ECS entity; only its [`Ball`] component matters to the
//! simulation. It steps cell-by-cell along the flood-filled route, always
//! hugging it clockwise and laying a `Trail` (3) behind it.
//!
//! Energy == accumulated distance, measured in cells moved:
//! * moving **down** accumulates `CHARGE_PER_CELL` per step,
//! * moving **up or level** (`dy >= 0`) consumes the same amount,
//! * charge hitting zero ends the run.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::grid::{CELL_PX, Cell, GRID_H, Grid, NEIGHBORS8};

/// Movement steps per second.
pub const STEPS_PER_SECOND: f32 = 6.0;
/// Charge gained or spent per cell moved.
pub const CHARGE_PER_CELL: f32 = 1.0;

/// Tunable run parameters. Defaults are for the real game; tests and manual
/// experiments can raise [`Tuning::start_charge`].
#[derive(Resource)]
pub struct Tuning {
    /// Charge the ball starts a run with. Zero by default, so the ball has to
    /// earn its energy; bump it to let a run start on flat or uphill ground.
    pub start_charge: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { start_charge: 0.0 }
    }
}

/// The ball. `cell` is its grid position, `dir` its last step direction.
#[derive(Component)]
pub struct Ball {
    pub cell: IVec2,
    pub dir: IVec2,
    pub charge: f32,
    /// Solid cell the ball is currently following. Keeps it on one contour
    /// when two lines run close enough for their surfaces to touch.
    pub anchor: Option<IVec2>,
    timer: f32,
}

impl Ball {
    pub fn new(cell: IVec2, charge: f32) -> Self {
        Self {
            cell,
            dir: IVec2::X,
            charge,
            anchor: None,
            timer: 0.0,
        }
    }
}

/// Marks the on-screen charge readout.
#[derive(Component)]
pub struct ChargeText;

/// How the current run is going.
#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum Outcome {
    #[default]
    Running,
    /// Returned to a previous cell with at least as much charge -> perpetual.
    Won,
    /// Nowhere legal to go, or charge dropped to zero.
    Stuck,
}

/// Per-run bookkeeping: first-arrival charge at each cell, and the flood-filled
/// route (connected component) the ball is confined to.
#[derive(Resource, Default)]
pub struct Run {
    pub visits: HashMap<IVec2, f32>,
    pub route: HashSet<IVec2>,
    pub outcome: Outcome,
}

/// Spawn the charge readout (once, at startup).
pub fn spawn_charge_text(commands: &mut Commands) {
    commands.spawn((
        Text2d::new("Charge: 0.0"),
        TextFont {
            font_size: FontSize::Px(19.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.85, 0.35)),
        Transform::from_xyz(0.0, GRID_H as f32 * CELL_PX / 2.0 - 40.0, 10.0),
        ChargeText,
    ));
}

/// Refresh the charge readout every frame. In pen mode there is no ball, so
/// it shows the configured starting charge instead.
pub fn update_charge_text(
    tuning: Res<Tuning>,
    balls: Query<&Ball>,
    mut texts: Query<&mut Text2d, With<ChargeText>>,
) {
    let charge = balls
        .iter()
        .next()
        .map_or(tuning.start_charge, |b| b.charge);
    for mut text in &mut texts {
        text.0 = format!("Charge: {charge:.1}");
    }
}

/// `[` / `]` nudge the starting charge while testing.
pub fn tune_start_charge(keys: Res<ButtonInput<KeyCode>>, mut tuning: ResMut<Tuning>) {
    if keys.just_pressed(KeyCode::BracketLeft) {
        tuning.start_charge = (tuning.start_charge - 1.0).max(0.0);
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        tuning.start_charge += 1.0;
    }
}

/// Spawn the ball sprite for a fresh run.
pub fn spawn_ball(commands: &mut Commands, grid: &Grid, start: IVec2, charge: f32) -> Entity {
    let pos = grid.cell_to_world(start);
    commands
        .spawn((
            Ball::new(start, charge),
            Sprite::from_color(Color::srgb(1.0, 0.55, 0.2), Vec2::splat(CELL_PX * 0.7)),
            Transform::from_xyz(pos.x, pos.y, 5.0),
        ))
        .id()
}

/// Advance every ball along its track.
pub fn step_ball(
    time: Res<Time>,
    mut grid: ResMut<Grid>,
    mut run: ResMut<Run>,
    mut balls: Query<&mut Ball>,
) {
    if run.outcome != Outcome::Running {
        return;
    }
    let interval = 1.0 / STEPS_PER_SECOND;
    let dt = time.delta_secs();
    for mut ball in &mut balls {
        ball.timer += dt;
        while ball.timer >= interval {
            ball.timer -= interval;
            if run.outcome != Outcome::Running {
                break;
            }
            step_once(&mut grid, &mut run, &mut ball);
        }
    }
}

/// One tile of movement.
fn step_once(grid: &mut Grid, run: &mut Run, ball: &mut Ball) {
    let from = ball.cell;
    let behind = from - ball.dir;

    // Pick the most clockwise (rightmost) neighbour on the route, never
    // doubling back and never cutting a wall corner. Fresh surface always
    // beats re-entering the trail.
    let mut best: Option<(f32, IVec2, bool, IVec2)> = None; // key, cell, is_trail, anchor
    for d in NEIGHBORS8 {
        let next = ball.cell + d;
        if next == behind
            || !grid.is_track(next)
            || grid.step_blocked(ball.cell, d)
            || !run.route.contains(&next)
        {
            continue;
        }

        // Stay on the contour we are already following: the solid shared with
        // the next cell has to be next to the current anchor.
        let shared = grid.common_solids(ball.cell, next);
        let anchor = match ball.anchor {
            None => match shared.first() {
                Some(solid) => *solid,
                None => continue,
            },
            Some(current) => match shared.iter().copied().find(|s| chebyshev(*s, current) <= 1)
            {
                Some(solid) => solid,
                None => continue,
            },
        };

        let is_trail = grid.get(next) == Some(Cell::Trail);
        let angle = turn(ball.dir.as_vec2(), d.as_vec2());
        // Prefer, in order: fresh surface, a diagonal step (go as far as
        // possible), clockwise/straight, then the most clockwise of what's left.
        let counterclockwise = if angle > 0.0 { 1.0 } else { 0.0 };
        let orthogonal = if d.x == 0 || d.y == 0 { 1.0 } else { 0.0 };
        let key = if is_trail { 1000.0 } else { 0.0 }
            + orthogonal * 100.0
            + counterclockwise * 10.0
            + (angle + std::f32::consts::PI) * 0.001;
        if best.is_none_or(|(bk, _, _, _)| key < bk) {
            best = Some((key, next, is_trail, anchor));
        }
    }

    let Some((_, next, is_trail, anchor)) = best else {
        run.outcome = Outcome::Stuck;
        info!("Ball stuck at {:?}: no track ahead", ball.cell);
        return;
    };
    ball.anchor = Some(anchor);

    if is_trail {
        let best_charge = run.visits.get(&next).copied().unwrap_or(f32::INFINITY);
        if ball.charge + f32::EPSILON < best_charge {
            run.outcome = Outcome::Stuck;
            info!(
                "Ball returned to {:?} with {:.1} < {:.1} charge — not self-sustaining",
                next, ball.charge, best_charge
            );
            return;
        }
        run.outcome = Outcome::Won;
        info!(
            "Eternal loop! Back at {:?} with {:.1} >= {:.1} charge",
            next, ball.charge, best_charge
        );
    }

    // Lay trail behind us and record the charge we arrived with.
    if grid.get(ball.cell) == Some(Cell::Surface) {
        grid.set(ball.cell, Cell::Trail);
    }
    run.visits.entry(ball.cell).or_insert(ball.charge);

    // Move, then settle the energy for this cell. Down accumulates; up and
    // level consume one cell's worth.
    let d = next - ball.cell;
    if d.y < 0 {
        ball.charge += CHARGE_PER_CELL;
    } else {
        ball.charge -= CHARGE_PER_CELL;
    }
    if ball.charge < 0.0 {
        ball.charge = 0.0;
        run.outcome = Outcome::Stuck;
        info!("Ball ran out of charge at {:?}", ball.cell);
    }
    ball.dir = d;
    ball.cell = next;
    run.visits.entry(next).or_insert(ball.charge);

    // If we cut a diagonal, also consume the two orthogonal "staircase" cells
    // between the endpoints, so the parallel representation of this contour
    // piece is filled too and no stray 2s are left behind. Those covered cells
    // count as distance travelled, so they feed the charge as well.
    if d.x != 0 && d.y != 0 {
        // Filled cells count as distance travelled and share the sign of the
        // move: a descending diagonal accumulates them, climbing/level consumes.
        let fill = if d.y < 0 { CHARGE_PER_CELL } else { -CHARGE_PER_CELL };
        for corner in [from + IVec2::new(d.x, 0), from + IVec2::new(0, d.y)] {
            if grid.is_track(corner) {
                grid.set(corner, Cell::Trail);
                ball.charge += fill;
            }
        }
        if ball.charge < 0.0 {
            ball.charge = 0.0;
            run.outcome = Outcome::Stuck;
            info!("Ball ran out of charge at {:?}", ball.cell);
        }
    }
}

/// Keep the sprite glued to its grid cell.
pub fn update_ball_transform(grid: Res<Grid>, mut balls: Query<(&Ball, &mut Transform)>) {
    for (ball, mut transform) in &mut balls {
        let pos = grid.cell_to_world(ball.cell);
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
    }
}

/// Signed turn from `from` to `to`: negative is clockwise (world +y is up).
fn turn(from: Vec2, to: Vec2) -> f32 {
    let dot = from.dot(to);
    let cross = from.x * to.y - from.y * to.x;
    cross.atan2(dot)
}

/// Chebyshev (king-move) distance between two cells.
fn chebyshev(a: IVec2, b: IVec2) -> i32 {
    (a.x - b.x).abs().max((a.y - b.y).abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests start with headroom so runs can begin on flat ground.
    const TEST_CHARGE: f32 = 10.0;

    fn grid() -> Grid {
        Grid::new(Handle::default())
    }

    /// Paint a filled disk of solids and return the ball's start.
    fn disk(grid: &mut Grid, r: f32) -> IVec2 {
        let (cx, cy) = (40.0f32, 40.0f32);
        for y in 0..80 {
            for x in 0..80 {
                if ((x as f32) - cx).hypot((y as f32) - cy) <= r {
                    grid.paint(IVec2::new(x, y), Cell::Solid);
                }
            }
        }
        grid.find_start().unwrap()
    }

    /// The ball must only ever step between cells that share a solid, so it
    /// follows one contour and never hops to a parallel one.
    #[test]
    fn painted_disk_contour_does_not_hop() {
        let mut grid = grid();
        let start = disk(&mut grid, 12.0);
        let mut run = Run::default();
        run.route = grid.reachable(start);
        run.visits.insert(start, TEST_CHARGE);
        let mut ball = Ball::new(start, TEST_CHARGE);
        let mut visited = HashSet::new();

        for _ in 0..80 {
            if run.outcome != Outcome::Running {
                break;
            }
            let from = ball.cell;
            step_once(&mut grid, &mut run, &mut ball);
            if run.outcome != Outcome::Running {
                break;
            }
            assert!(
                grid.shares_solid(from, ball.cell),
                "hopped from {from:?} to {:?}",
                ball.cell
            );
            visited.insert(ball.cell);
        }
        assert!(visited.len() > 10, "ball barely moved: {}", visited.len());
    }

    /// Two parallel lines one cell apart must not let the ball hop across at
    /// the ends, where each line's surface touches the other's terminal solid.
    #[test]
    fn parallel_lines_do_not_hop() {
        let mut grid = grid();
        for y in 10..=20 {
            grid.set(IVec2::new(0, y), Cell::Solid);
            grid.set(IVec2::new(2, y), Cell::Solid);
            grid.set(IVec2::new(1, y), Cell::Surface);
        }
        for x in 0..=2 {
            grid.set(IVec2::new(x, 9), Cell::Surface);
            grid.set(IVec2::new(x, 21), Cell::Surface);
        }

        let mut run = Run::default();
        run.route = grid.reachable(IVec2::new(1, 20));
        run.visits.insert(IVec2::new(1, 20), TEST_CHARGE);
        let mut ball = Ball::new(IVec2::new(1, 20), TEST_CHARGE);
        ball.dir = IVec2::new(0, 1); // heading up
        ball.anchor = Some(IVec2::new(0, 20)); // following the left line

        step_once(&mut grid, &mut run, &mut ball);
        assert!(ball.cell.x <= 1, "hopped to the right line: {:?}", ball.cell);
    }

    /// A dead-end line must stop the ball, never send it back the way it came.
    #[test]
    fn dead_end_does_not_reverse() {
        let mut grid = grid();
        for x in 5..=10 {
            grid.set(IVec2::new(x, 5), Cell::Surface);
            grid.set(IVec2::new(x, 6), Cell::Solid);
        }
        let start = grid.find_start().unwrap();
        assert_eq!(start, IVec2::new(5, 5));

        let mut run = Run::default();
        run.route = grid.reachable(start);
        run.visits.insert(start, TEST_CHARGE);
        let mut ball = Ball::new(start, TEST_CHARGE);

        for _ in 0..20 {
            if run.outcome != Outcome::Running {
                break;
            }
            step_once(&mut grid, &mut run, &mut ball);
        }
        assert_eq!(run.outcome, Outcome::Stuck);
        assert_eq!(ball.cell, IVec2::new(10, 5));
        assert_eq!(ball.dir, IVec2::X); // still facing forward
    }

    /// A diagonal step also consumes the orthogonal cells that form the
    /// staircase alternative for that step.
    #[test]
    fn diagonal_step_fills_the_staircase() {
        let mut grid = grid();
        grid.set(IVec2::new(0, 1), Cell::Solid);
        grid.set(IVec2::new(0, 0), Cell::Surface);
        grid.set(IVec2::new(1, 0), Cell::Surface);
        grid.set(IVec2::new(1, 1), Cell::Surface);

        let mut run = Run::default();
        run.route = grid.reachable(IVec2::new(0, 0));
        run.visits.insert(IVec2::new(0, 0), TEST_CHARGE);
        let mut ball = Ball::new(IVec2::new(0, 0), TEST_CHARGE);
        ball.dir = IVec2::new(0, 1);

        step_once(&mut grid, &mut run, &mut ball);
        assert_eq!(ball.cell, IVec2::new(1, 1), "should take the diagonal");
        assert_eq!(grid.get(IVec2::new(1, 0)), Some(Cell::Trail));
        // Climbing diagonal (spend 1) plus the consumed fill (spend 1).
        assert_eq!(ball.charge, TEST_CHARGE - 2.0);
    }

    /// Cells filled in by a diagonal step are counted as distance travelled, so
    /// they add to the charge pool.
    #[test]
    fn filling_the_staircase_adds_charge() {
        let mut grid = grid();
        grid.set(IVec2::new(10, 10), Cell::Solid);
        grid.set(IVec2::new(10, 11), Cell::Surface);
        grid.set(IVec2::new(11, 11), Cell::Surface);
        grid.set(IVec2::new(11, 10), Cell::Surface);

        let mut run = Run::default();
        run.route = grid.reachable(IVec2::new(10, 11));
        run.visits.insert(IVec2::new(10, 11), TEST_CHARGE);
        let mut ball = Ball::new(IVec2::new(10, 11), TEST_CHARGE);
        ball.dir = IVec2::new(0, 1);

        step_once(&mut grid, &mut run, &mut ball);
        // Down-right diagonal (+1) plus the filled (11, 11) (+1).
        assert_eq!(ball.cell, IVec2::new(11, 10));
        assert_eq!(grid.get(IVec2::new(11, 11)), Some(Cell::Trail));
        assert_eq!(ball.charge, TEST_CHARGE + 2.0);
    }

    /// On a drawn diagonal, the ball should take diagonal steps rather than
    /// staircasing (each level step is pure charge loss).
    #[test]
    fn diagonal_stroke_prefers_diagonal_steps() {
        let mut grid = grid();
        for i in 0..20 {
            grid.paint(IVec2::new(10 + i, 10 + i), Cell::Solid);
        }
        let start = grid.find_start().unwrap();
        let mut run = Run::default();
        run.route = grid.reachable(start);
        run.visits.insert(start, TEST_CHARGE);
        let mut ball = Ball::new(start, TEST_CHARGE);

        let (mut diagonal, mut orthogonal) = (0, 0);
        for _ in 0..40 {
            if run.outcome != Outcome::Running {
                break;
            }
            let from = ball.cell;
            step_once(&mut grid, &mut run, &mut ball);
            if run.outcome != Outcome::Running {
                break;
            }
            let d = ball.cell - from;
            if d.x != 0 && d.y != 0 {
                diagonal += 1;
            } else {
                orthogonal += 1;
            }
        }
        assert!(
            diagonal > orthogonal,
            "expected mostly diagonal moves, got diagonal={diagonal} orthogonal={orthogonal}"
        );
    }
}
