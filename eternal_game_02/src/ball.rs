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

use crate::grid::{CELL_PX, Cell, GRID_H, Grid, NEIGHBORS4, NEIGHBORS8};

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
    /// When true the ball only steps on `N` instead of on a timer.
    pub manual: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            start_charge: 0.0,
            manual: false,
        }
    }
}

/// The ball. `cell` is its grid position, `dir` its last step direction.
#[derive(Component)]
pub struct Ball {
    pub cell: IVec2,
    pub dir: IVec2,
    pub charge: f32,
    /// Connected solid mass the ball is following. Keeps it on one contour
    /// when two lines run close enough for their surfaces to touch.
    pub component: Option<usize>,
    /// Whether the ball has made a real move yet (so the first heading isn't
    /// mistaken for a previous direction when detecting turn combos).
    moved: bool,
    timer: f32,
}

impl Ball {
    pub fn new(cell: IVec2, charge: f32) -> Self {
        Self {
            cell,
            dir: IVec2::X,
            charge,
            component: None,
            moved: false,
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
/// One step the ball took, kept for the on-screen itinerary.
#[derive(Clone, Copy, Debug)]
pub struct MoveRecord {
    pub cell: IVec2,
    pub dir: IVec2,
    /// True when this was a combo (a horizontal converted by the preceding
    /// vertical). Kept for tests/inspection; colour only uses `charge`.
    #[allow(dead_code)]
    pub combo: bool,
    /// Charge this move added (positive accumulates, negative consumes).
    pub charge: f32,
}

/// Per-run bookkeeping: first-arrival charge at each cell, the flood-filled
/// route, the solid components, and the itinerary of moves taken.
#[derive(Resource, Default)]
pub struct Run {
    pub visits: HashMap<IVec2, f32>,
    pub route: HashSet<IVec2>,
    /// Every solid cell labelled with its 8-connected component id.
    pub components: HashMap<IVec2, usize>,
    pub itinerary: Vec<MoveRecord>,
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
        Visibility::Hidden,
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
        let mode = if tuning.manual { "MANUAL [N]" } else { "auto [M]" };
        text.0 = format!("Charge: {charge:.1}   {mode}");
    }
}

/// `[` / `]` nudge the starting charge while testing.
pub fn tune_start_charge(
    keys: Res<ButtonInput<KeyCode>>,
    debug: Res<crate::paint::Debug>,
    mut tuning: ResMut<Tuning>,
) {
    if !debug.0 {
        return;
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        tuning.start_charge = (tuning.start_charge - 1.0).max(0.0);
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        tuning.start_charge += 1.0;
    }
    if keys.just_pressed(KeyCode::KeyM) {
        tuning.manual = !tuning.manual;
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
    tuning: Res<Tuning>,
    mut grid: ResMut<Grid>,
    mut run: ResMut<Run>,
    mut balls: Query<&mut Ball>,
) {
    // In manual mode the ball only moves on `N` (see `manual_step`).
    if run.outcome != Outcome::Running || tuning.manual {
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

/// Set up a fresh run on `grid` starting at `start`.
pub fn start_run(run: &mut Run, grid: &Grid, start: IVec2, charge: f32) {
    run.route = grid.reachable(start);
    run.components = grid.solid_components();
    run.visits.clear();
    run.visits.insert(start, charge);
    run.itinerary.clear();
    run.outcome = Outcome::Running;
}

/// One tile of movement. Public so the headless replay can drive it.
pub(crate) fn step_once(grid: &mut Grid, run: &mut Run, ball: &mut Ball) {
    let from = ball.cell;

    // Work out which connected solid mass we are following (first move only).
    if ball.component.is_none() {
        ball.component = NEIGHBORS8
            .iter()
            .find_map(|offset| run.components.get(&(from + *offset)).copied());
        if ball.component.is_none() {
            run.outcome = Outcome::Stuck;
            info!("Ball at {from:?} has no adjacent solid to follow");
            return;
        }
    }
    let component = ball.component.unwrap();
    let previous_dir = ball.moved.then_some(ball.dir);
    // On the first move there is no "behind" yet, and the heading is chosen
    // from the solid so the ball always sets off clockwise (solid on the right).
    let heading = previous_dir.unwrap_or_else(|| initial_heading(run, component, from));
    let behind = previous_dir.map(|d| from - d);

    // Movement is orthogonal only. Follow the contour with a right-hand rule:
    // take a right turn (clockwise) if one exists, else go straight, else left,
    // never reversing.
    let mut best: Option<(f32, IVec2, bool)> = None; // key, cell, is_trail
    for d in NEIGHBORS4 {
        let next = ball.cell + d;
        if Some(next) == behind || !grid.is_track(next) || !run.route.contains(&next) {
            continue;
        }

        // Stay on the solid component we are following: the destination must
        // still hug that same mass.
        let on_component = NEIGHBORS8
            .iter()
            .any(|offset| run.components.get(&(next + *offset)) == Some(&component));
        if !on_component {
            continue;
        }

        let is_trail = grid.get(next) == Some(Cell::Trail);
        let angle = turn(heading.as_vec2(), d.as_vec2());
        // Prefer right (clockwise) > straight > left > reverse. The reverse can
        // only happen on the first move (there is no "behind" yet), and
        // atan2(-0.0, -1.0) = -pi would otherwise score it as most clockwise.
        let counterclockwise = if angle > 0.0 { 1.0 } else { 0.0 };
        let reverse = if angle.abs() > std::f32::consts::FRAC_PI_2 + 0.1 {
            1.0
        } else {
            0.0
        };
        let key = if is_trail { 1000.0 } else { 0.0 }
            + reverse * 20.0
            + counterclockwise * 10.0
            + (angle + std::f32::consts::PI) * 0.001;
        if best.is_none_or(|(bk, _, _)| key < bk) {
            best = Some((key, next, is_trail));
        }
    }

    let Some((_, next, is_trail)) = best else {
        run.outcome = Outcome::Stuck;
        info!("Ball stuck at {:?}: no track ahead", ball.cell);
        return;
    };

    // A `Trail` cell is only a loop-closure if the ball actually walked it
    // (recorded in `visits`). Cells filled in by a turn combo are `Trail` too,
    // but were never visited, so they are just passed through.
    if is_trail
        && let Some(best_charge) = run.visits.get(&next).copied()
    {
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

    // Move charge. Verticals are signed by direction. A horizontal is level, so
    // it consumes by default (-1), but a horizontal right after a descent is
    // converted to a gain (+1) — the combo.
    let d = next - ball.cell;
    let after_vertical = previous_dir.is_some_and(|p| p.y != 0);
    let combo = d.y == 0 && after_vertical;
    let charge = if d.y < 0 {
        CHARGE_PER_CELL
    } else if d.y > 0 {
        -CHARGE_PER_CELL
    } else if previous_dir.is_some_and(|p| p.y < 0) {
        CHARGE_PER_CELL
    } else {
        -CHARGE_PER_CELL
    };
    ball.charge += charge;
    if ball.charge < 0.0 {
        ball.charge = 0.0;
        run.outcome = Outcome::Stuck;
        info!("Ball ran out of charge at {:?}", ball.cell);
    }
    ball.dir = d;
    ball.cell = next;
    ball.moved = true;
    run.visits.entry(next).or_insert(ball.charge);
    run.itinerary.push(MoveRecord {
        cell: from,
        dir: d,
        combo,
        charge,
    });
}

/// Colour the ball by its charge, battery-style: grey when depleted, waxing
/// through blue to green as it fills. Charge is unbounded, so `t` saturates.
pub fn update_ball_color(mut balls: Query<(&Ball, &mut Sprite)>) {
    for (ball, mut sprite) in &mut balls {
        sprite.color = charge_color(ball.charge);
    }
}

fn charge_color(charge: f32) -> Color {
    let t = (charge / (charge + 8.0)).clamp(0.0, 1.0);
    let hue = 210.0 - 90.0 * t; // blue -> green
    let saturation = 0.9 * t;
    let lightness = 0.40 + 0.25 * t;
    Color::hsl(hue, saturation, lightness)
}

/// Keep the sprite glued to its grid cell.
pub fn update_ball_transform(grid: Res<Grid>, mut balls: Query<(&Ball, &mut Transform)>) {
    for (ball, mut transform) in &mut balls {
        let pos = grid.cell_to_world(ball.cell);
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
    }
}

/// In manual mode, one step per press of `N`.
pub fn manual_step(
    keys: Res<ButtonInput<KeyCode>>,
    tuning: Res<Tuning>,
    mut grid: ResMut<Grid>,
    mut run: ResMut<Run>,
    mut balls: Query<&mut Ball>,
) {
    if !tuning.manual || run.outcome != Outcome::Running || !keys.just_pressed(KeyCode::KeyN) {
        return;
    }
    for mut ball in &mut balls {
        step_once(&mut grid, &mut run, &mut ball);
    }
}

/// Draw the movement itinerary: an arrow at every cell the ball left, coloured
/// purely by what the step did to the charge — green accumulates, orange
/// consumes. A horizontal is just +1 (green) or -1 (orange) like anything else.
pub fn draw_itinerary(run: Res<Run>, grid: Res<Grid>, mut gizmos: Gizmos) {
    for m in &run.itinerary {
        let start = grid.cell_to_world(m.cell);
        let end = start + m.dir.as_vec2() * (CELL_PX * 0.9);
        let color = if m.charge > 0.0 {
            Color::srgb(0.30, 0.85, 0.35)
        } else {
            Color::srgb(1.0, 0.55, 0.10)
        };
        gizmos
            .arrow_2d(start, end, color)
            .with_tip_length(CELL_PX * 0.45);
    }
}

/// Signed turn from `from` to `to`: negative is clockwise (world +y is up).
fn turn(from: Vec2, to: Vec2) -> f32 {
    let dot = from.dot(to);
    let cross = from.x * to.y - from.y * to.x;
    cross.atan2(dot)
}

/// Pick the starting heading so the solid sits on the ball's right (clockwise).
fn initial_heading(run: &Run, component: usize, cell: IVec2) -> IVec2 {
    let mut best = IVec2::X;
    let mut best_score = -1;
    for h in NEIGHBORS4 {
        // "Right" of heading `h` is `h` rotated clockwise 90 degrees.
        let right = IVec2::new(h.y, -h.x);
        let probes = [cell + right, cell + h + right, cell - h + right];
        let score = probes
            .iter()
            .filter(|c| run.components.get(c) == Some(&component))
            .count() as i32;
        if score > best_score {
            best_score = score;
            best = h;
        }
    }
    best
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

    /// The ball must always hug the same connected solid mass and never hop to
    /// a parallel contour.
    #[test]
    fn painted_disk_contour_does_not_hop() {
        let mut grid = grid();
        let start = disk(&mut grid, 12.0);
        let labels = grid.solid_components();
        let component = *labels.values().next().unwrap();
        let mut run = Run::default();
        run.route = grid.reachable(start);
        run.components = labels;
        run.visits.insert(start, TEST_CHARGE);
        let mut ball = Ball::new(start, TEST_CHARGE);
        let mut visited = HashSet::new();

        for _ in 0..80 {
            if run.outcome != Outcome::Running {
                break;
            }
            step_once(&mut grid, &mut run, &mut ball);
            if run.outcome != Outcome::Running {
                break;
            }
            assert!(
                NEIGHBORS8
                    .iter()
                    .any(|d| run.components.get(&(ball.cell + *d)) == Some(&component)),
                "ball left the disk contour at {:?}",
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
        run.components = grid.solid_components();
        run.visits.insert(IVec2::new(1, 20), TEST_CHARGE);
        let mut ball = Ball::new(IVec2::new(1, 20), TEST_CHARGE);
        ball.dir = IVec2::new(0, 1); // heading up
        ball.component = run.components.get(&IVec2::new(0, 20)).copied(); // left line

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
        run.components = grid.solid_components();
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

    /// A horizontal move takes the sign of the vertical move before it: here an
    /// up step (-1) is followed by a horizontal converted to -1 (a combo).
    #[test]
    fn horizontal_takes_preceding_sign() {
        let mut grid = grid();
        grid.set(IVec2::new(5, 5), Cell::Solid);
        grid.set(IVec2::new(4, 5), Cell::Surface);
        grid.set(IVec2::new(4, 6), Cell::Surface);
        grid.set(IVec2::new(5, 6), Cell::Surface);

        let mut run = Run::default();
        run.route = grid.reachable(IVec2::new(4, 5));
        run.components = grid.solid_components();
        run.visits.insert(IVec2::new(4, 5), TEST_CHARGE);
        let mut ball = Ball::new(IVec2::new(4, 5), TEST_CHARGE);
        ball.dir = IVec2::new(0, 1);

        step_once(&mut grid, &mut run, &mut ball); // up: -1
        step_once(&mut grid, &mut run, &mut ball); // right after up: combo -1
        assert_eq!(ball.charge, TEST_CHARGE - 2.0);
        let last = run.itinerary.last().expect("a move");
        assert!(last.combo && last.charge < 0.0, "up-then-right is a costly combo");
    }

    /// Flat ground (horizontal with no preceding vertical) still consumes.
    #[test]
    fn flat_ground_costs() {
        let mut grid = grid();
        for x in 5..=10 {
            grid.set(IVec2::new(x, 5), Cell::Surface);
            grid.set(IVec2::new(x, 6), Cell::Solid);
        }
        let mut run = Run::default();
        run.route = grid.reachable(IVec2::new(5, 5));
        run.components = grid.solid_components();
        run.visits.insert(IVec2::new(5, 5), TEST_CHARGE);
        let mut ball = Ball::new(IVec2::new(5, 5), TEST_CHARGE);

        step_once(&mut grid, &mut run, &mut ball);
        assert!(!run.itinerary.last().unwrap().combo);
        assert_eq!(ball.charge, TEST_CHARGE - 1.0);
    }

    /// With no orthogonal move available the ball stops and never reverses.
    #[test]
    fn stuck_when_no_orthogonal_move() {
        let mut grid = grid();
        grid.set(IVec2::new(10, 10), Cell::Solid);
        grid.set(IVec2::new(10, 11), Cell::Surface);
        grid.set(IVec2::new(11, 10), Cell::Surface);

        let mut run = Run::default();
        run.route = grid.reachable(IVec2::new(10, 11));
        run.components = grid.solid_components();
        run.visits.insert(IVec2::new(10, 11), TEST_CHARGE);
        let mut ball = Ball::new(IVec2::new(10, 11), TEST_CHARGE);
        ball.dir = IVec2::new(0, 1);

        step_once(&mut grid, &mut run, &mut ball);
        assert_eq!(run.outcome, Outcome::Stuck);
    }
}

