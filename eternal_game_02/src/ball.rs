//! Phase 2: the ball that walks the generated surface.
//!
//! The ball is a plain ECS entity; only its [`Ball`] component matters to the
//! simulation. It steps cell-by-cell clockwise around a solid anchor, laying no
//! trail: `(cell, anchor)` is the whole state.
//!
//! Energy == accumulated distance, measured in cells moved:
//! * moving **down** accumulates `CHARGE_PER_CELL` per step,
//! * moving **up or level** (`dy >= 0`) consumes the same amount,
//! * charge hitting zero ends the run.

use std::collections::{HashMap, VecDeque};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::grid::{CELL_PX, Cell, GRID_H, Grid, NEIGHBORS4, NEIGHBORS8};

/// Movement steps per second.
pub const STEPS_PER_SECOND: f32 = 6.0;
/// Charge gained or spent per cell moved.
pub const CHARGE_PER_CELL: f32 = 1.0;
/// Each completed lap speeds the ball up by this factor, up to [`MAX_SPEED`],
/// so a self-sustaining loop visibly accelerates forever.
pub const SPEEDUP_PER_LAP: f32 = 1.35;
/// Cap on [`Run::speed`], so an eternal loop stays playable.
pub const MAX_SPEED: f32 = 25.0;
/// How far the shell turns per cell, as a fraction of a full turn. The shell's
/// angular speed is this times the step rate, so it automatically speeds up as
/// the ball accelerates; this base is kept low so the default speed is calm.
pub const SHELL_ROLL_PER_CELL: f32 = 0.15;
/// Maximum turn of the inner core toward the surface normal per cell moved, in
/// degrees. At 90 degrees a right-angle corner settles in about three cells.
pub const CORE_ALIGN_PER_CELL_DEG: f32 = 30.0;
/// The same limit in radians.
const CORE_ALIGN_PER_CELL: f32 = CORE_ALIGN_PER_CELL_DEG * std::f32::consts::PI / 180.0;
/// The ball's radius in world units. Kept just under half a cell, so it
/// nestles against the solid it hugs while still reading as a ball.
pub const BALL_RADIUS: f32 = CELL_PX * 0.44;
/// On-screen diameter of the ball, in world units. The three square textures
/// are [`BALL_TEX_PX`]px and map onto this.
pub const BALL_DIAMETER: f32 = BALL_RADIUS * 2.0;
/// Edge length of the square ball textures, in pixels.
const BALL_TEX_PX: f32 = 80.0;
/// Horizontal spacing between the three battery bays, in texture pixels.
const BAY_DX_PX: f32 = 14.0;
/// A battery's centre height above the ball centre, in texture pixels.
const BATT_Y_PX: f32 = 16.0;
/// A battery's size in texture pixels (width, height).
const BATT_PX: Vec2 = Vec2::new(12.0, 14.0);
/// Completed laps a solved loop survives before it overloads and takes the
/// whole level with it — the victory.
pub const VICTORY_LAPS: u32 = 8;
/// Longest arrow history kept, so a run that loops forever does not grow
/// without bound. Older moves fall off the back.
const MAX_ITINERARY: usize = 1024;

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

/// The ball. `cell` is its grid position, `dir` its last step direction, and
/// `anchor` the solid cell it is currently stuck to (its "stick point"). The
/// next move is a pure function of `(cell, anchor)`.
#[derive(Component)]
pub struct Ball {
    pub cell: IVec2,
    pub dir: IVec2,
    pub charge: f32,
    /// The solid cell the ball is stuck to. `None` until the first step
    /// resolves it from the adjacent solids.
    pub anchor: Option<IVec2>,
    /// Whether the ball has made a real move yet (so the first heading isn't
    /// mistaken for a previous direction when detecting turn combos).
    moved: bool,
    /// The cell the current move started from, so the sprite can be smoothly
    /// interpolated from there to [`Ball::cell`].
    prev: IVec2,
    /// Roll angle at the start of the current move, in radians.
    spin: f32,
    /// Roll added over the current move (a fraction of a turn, see
    /// [`SHELL_ROLL_PER_CELL`]).
    spin_delta: f32,
    /// Core angle at the start of the current move, in radians.
    core_angle: f32,
    /// How far the core turns over the current move: a small step toward the
    /// surface normal, so it eases over many cells.
    core_delta: f32,
    timer: f32,
}

impl Ball {
    pub fn new(cell: IVec2, charge: f32) -> Self {
        Self {
            cell,
            dir: IVec2::X,
            charge,
            anchor: None,
            moved: false,
            prev: cell,
            spin: 0.0,
            spin_delta: 0.0,
            core_angle: 0.0,
            core_delta: 0.0,
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
    /// Nowhere legal to go.
    Stuck,
    /// Tried to move but the battery was empty. The ball explodes (see
    /// [`crate::explosion`]).
    Depleted,
    /// A self-sustaining loop ran long enough to overload. The whole level
    /// detonates: the victory.
    Victory,
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

/// Per-run bookkeeping: the first-arrival charge at each `(cell, anchor)`
/// state, the solid components, the itinerary of moves taken, and how many
/// laps the ball has completed.
#[derive(Resource)]
pub struct Run {
    pub visits: HashMap<(IVec2, IVec2), f32>,
    /// Every solid cell labelled with its 8-connected component id.
    pub components: HashMap<IVec2, usize>,
    pub itinerary: Vec<MoveRecord>,
    pub outcome: Outcome,
    /// Set for one frame when `Tab` enters run mode, so that first press only
    /// takes manual control instead of also nudging the ball.
    pub just_entered: bool,
    /// The `(cell, anchor)` state where the ball first closed its loop. Each
    /// time it returns here a lap is complete. `None` until then.
    pub closure: Option<(IVec2, IVec2)>,
    /// True once a loop closure met its charge guarantee — the level is solved.
    pub solved: bool,
    /// Laps completed since the run began.
    pub laps: u32,
    /// Step-rate multiplier. Grows every lap, so an eternal loop accelerates.
    pub speed: f32,
    /// Total moves made since the run began, including ones dropped from the
    /// front of [`Run::itinerary`]. Drives the itinerary arrow sprites.
    pub total_moves: u64,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            visits: HashMap::new(),
            components: HashMap::new(),
            itinerary: Vec::new(),
            outcome: Outcome::default(),
            just_entered: false,
            closure: None,
            solved: false,
            laps: 0,
            speed: 1.0,
            total_moves: 0,
        }
    }
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

/// The ball's three texture layers. The shell rolls, the core aligns to the
/// surface, and the batteries show the charge.
#[derive(Resource)]
pub struct BallTextures {
    pub shell: Handle<Image>,
    pub core: Handle<Image>,
    pub batteries: Handle<Image>,
}

/// Marker for the rolling outer tyre.
#[derive(Component)]
pub struct BallShell;

/// Marker for the stabilised inner chassis. It is rotated to align with the
/// surface normal so the batteries mounted on it always face away from the
/// ground.
#[derive(Component)]
pub struct BallCore;

/// Marker for a charge-tinted battery sprite.
#[derive(Component)]
pub struct BallBattery;

/// Load the ball textures once at startup, nearest-filtered so the pixels stay
/// crisp when the camera zooms in.
pub fn setup_ball_texture(mut commands: Commands, assets: Res<AssetServer>) {
    let nearest = |path: &'static str| -> Handle<Image> {
        assets
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| {
                settings.sampler = ImageSampler::nearest();
            })
            .load(path)
    };
    commands.insert_resource(BallTextures {
        shell: nearest("sprites/ball_shell.png"),
        core: nearest("sprites/ball_core.png"),
        batteries: nearest("sprites/battery_panel.png"),
    });
}

/// Average direction from `cell` toward its adjacent solid cells — i.e. toward
/// the surface the ball hugs. Falls back to straight down when there is none.
fn surface_normal(grid: &Grid, cell: IVec2) -> Vec2 {
    let mut sum = Vec2::ZERO;
    for offset in NEIGHBORS8 {
        if grid.get(cell + offset) == Some(Cell::Solid) {
            sum += offset.as_vec2();
        }
    }
    if sum.length_squared() > f32::EPSILON {
        sum.normalize()
    } else {
        Vec2::new(0.0, -1.0)
    }
}

/// Spawn the ball for a fresh run: a parent that carries the position, with the
/// rolling shell and an inner core that carries three charge-tinted batteries.
/// The core is rotated to the surface normal each frame, so the batteries stay
/// on the side away from the ground.
pub fn spawn_ball(
    commands: &mut Commands,
    grid: &Grid,
    textures: &BallTextures,
    start: IVec2,
    charge: f32,
) -> Entity {
    let pos = grid.cell_to_world(start);
    let px = BALL_DIAMETER / BALL_TEX_PX;
    let ball = commands
        .spawn((
            Ball::new(start, charge),
            Transform::from_xyz(pos.x, pos.y, 5.0),
        ))
        .id();
    commands.entity(ball).with_children(|parent| {
        parent.spawn((
            BallShell,
            Sprite {
                image: textures.shell.clone(),
                custom_size: Some(Vec2::splat(BALL_DIAMETER)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
        parent
            .spawn((
                BallCore,
                Sprite {
                    image: textures.core.clone(),
                    custom_size: Some(Vec2::splat(BALL_DIAMETER)),
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, 0.05),
            ))
            .with_children(|core| {
                for bay in [-1.0_f32, 0.0, 1.0] {
                    core.spawn((
                        BallBattery,
                        Sprite {
                            image: textures.batteries.clone(),
                            custom_size: Some(BATT_PX * px),
                            ..default()
                        },
                        Transform::from_xyz(bay * BAY_DX_PX * px, BATT_Y_PX * px, 0.01),
                    ));
                }
            });
    });
    ball
}

/// Advance every ball along its track.
pub fn step_ball(
    time: Res<Time>,
    tuning: Res<Tuning>,
    grid: Res<Grid>,
    mut run: ResMut<Run>,
    mut balls: Query<&mut Ball>,
) {
    // In manual mode the ball only moves on `N` (see `manual_step`).
    if run.outcome != Outcome::Running || tuning.manual {
        return;
    }
    // Each completed lap raises `speed`, so a solved loop gets faster and
    // faster. The cap keeps it finite.
    let interval = 1.0 / (STEPS_PER_SECOND * run.speed.max(0.01));
    let dt = time.delta_secs();
    for mut ball in &mut balls {
        ball.timer += dt;
        while ball.timer >= interval {
            ball.timer -= interval;
            if run.outcome != Outcome::Running {
                break;
            }
            step_once(&grid, &mut run, &mut ball);
        }
    }
}

/// Set up a fresh run on `grid`. The ball's anchor is resolved lazily on its
/// first step (it needs the solid components).
pub fn start_run(run: &mut Run, grid: &Grid) {
    run.components = grid.solid_components();
    run.visits.clear();
    run.itinerary.clear();
    run.outcome = Outcome::Running;
    run.closure = None;
    run.solved = false;
    run.laps = 0;
    run.speed = 1.0;
    run.total_moves = 0;
}

/// One tile of movement. Public so the headless replay can drive it.
pub(crate) fn step_once(grid: &Grid, run: &mut Run, ball: &mut Ball) {
    let from = ball.cell;

    // The anchor is the solid the ball is stuck to. Resolve it on the first
    // step (clockwise, so the solid sits on the ball's right) and record the
    // starting state so the loop condition can see it.
    if ball.anchor.is_none() {
        let Some(anchor) = initial_anchor(run, from) else {
            run.outcome = Outcome::Stuck;
            info!("Ball at {from:?} has no adjacent solid to follow");
            return;
        };
        ball.anchor = Some(anchor);
        run.visits.entry((from, anchor)).or_insert(ball.charge);
    }

    // Follow the contour clockwise: step to the next cell around the anchor.
    // If that cell is the anchor's own solid, the ball pivots its anchor to
    // that corner and retries. The move is a pure function of `(cell, anchor)`,
    // so no direction history (and no dead-end bounce) is needed.
    let mut anchor = ball.anchor.unwrap();
    let mut next = from;
    let mut stepped = false;
    for _ in 0..NEIGHBORS8.len() {
        let target = anchor + cw45(next - anchor);
        match grid.get(target) {
            Some(Cell::Solid) => anchor = target, // pivot around the corner
            Some(Cell::Empty) => {
                next = target;
                stepped = true;
                break;
            }
            _ => break,
        }
    }
    if !stepped {
        run.outcome = Outcome::Stuck;
        info!("Ball stuck at {from:?}: no track around the anchor");
        return;
    }
    ball.anchor = Some(anchor);

    let previous_dir = ball.moved.then_some(ball.dir);

    // Work out what the move costs before committing. Verticals are signed by
    // direction; a horizontal is level, so it consumes by default (-1), but a
    // horizontal right after a descent is converted to a gain (+1) — the combo.
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

    // The battery cannot pay for this step: the ball tries to move but there is
    // nothing left in it, so it bursts on the spot. Rolling downhill (which
    // gains charge) is still allowed from an empty battery.
    if ball.charge + charge < 0.0 {
        run.outcome = Outcome::Depleted;
        info!(
            "Ball at {:?} tried to move with {:.1} charge left — boom",
            ball.cell, ball.charge
        );
        return;
    }
    let arrival = ball.charge + charge;

    // Re-entering a `(cell, anchor)` state is the loop closure: the next move
    // is deterministic, so it must repeat from here. Meeting the charge
    // guarantee solves the level; missing it leaves the ball doomed.
    if let Some(best_charge) = run.visits.get(&(next, anchor)).copied() {
        run.closure.get_or_insert((next, anchor));
        if arrival + f32::EPSILON >= best_charge && !run.solved {
            run.solved = true;
            info!("Loop closed at {next:?}: the level is solved");
        }
    }

    // Set up this move's rolling animation: a full signed turn from the cell
    // being left to the one being entered, around the normal of the surface
    // the ball hugs. Because the solid is always on the ball's right,
    // `cross(direction, normal)` gives a consistent roll sign around a loop.
    let normal = surface_normal(grid, ball.cell);
    let roll = (d.x as f32 * normal.y - d.y as f32 * normal.x).signum();
    ball.spin = (ball.spin + ball.spin_delta).rem_euclid(std::f32::consts::TAU);
    ball.spin_delta = roll * std::f32::consts::TAU * SHELL_ROLL_PER_CELL;

    // Ease the inner core toward the destination's surface normal by at most a
    // fixed step per cell, so a corner is reached over many transitions rather
    // than snapped to.
    ball.core_angle = wrap_pi(ball.core_angle + ball.core_delta);
    let goal_normal = surface_normal(grid, next);
    let goal = goal_normal.x.atan2(-goal_normal.y);
    ball.core_delta =
        wrap_pi(goal - ball.core_angle).clamp(-CORE_ALIGN_PER_CELL, CORE_ALIGN_PER_CELL);

    ball.prev = ball.cell;
    ball.charge = arrival;
    ball.dir = d;
    ball.cell = next;
    ball.moved = true;
    run.visits.entry((next, anchor)).or_insert(arrival);
    run.itinerary.push(MoveRecord {
        cell: from,
        dir: d,
        combo,
        charge,
    });
    run.total_moves += 1;

    // Returning to the loop-closure state completes a lap. Every lap runs
    // faster, so a solved loop visibly accelerates toward its eternal state.
    if run.closure == Some((next, anchor)) {
        run.laps += 1;
        run.speed = (run.speed * SPEEDUP_PER_LAP).min(MAX_SPEED);
        info!("Lap {} at {next:?}: speed x{:.2}", run.laps, run.speed);
        // An eternal (solved) loop eventually overloads and takes the whole
        // level with it. A doomed loop never gets here — it has no solution.
        if run.solved && run.laps >= VICTORY_LAPS && run.outcome == Outcome::Running {
            run.outcome = Outcome::Victory;
            info!("Eternal loop overloaded — the whole level detonates!");
        }
    }

    // Bound the arrow history now that a run can loop indefinitely.
    let excess = run.itinerary.len().saturating_sub(MAX_ITINERARY);
    if excess > 0 {
        run.itinerary.drain(..excess);
    }
}

/// Colour the ball by its charge, battery-style: red when the battery is
/// empty (the ball is about to die), sweeping through orange and yellow to
/// green as it charges. Charge is unbounded, so `t` saturates.
pub fn update_ball_color(
    balls: Query<&Ball>,
    cores: Query<&ChildOf, With<BallCore>>,
    mut batteries: Query<(&ChildOf, &mut Sprite), With<BallBattery>>,
) {
    for (child_of, mut sprite) in &mut batteries {
        // The batteries hang off the core, which hangs off the ball.
        let charge = cores
            .get(child_of.parent())
            .ok()
            .and_then(|core| balls.get(core.parent()).ok())
            .map_or(0.0, |ball| ball.charge);
        sprite.color = charge_color(charge);
    }
}

fn charge_color(charge: f32) -> Color {
    let t = (charge / (charge + 8.0)).clamp(0.0, 1.0);
    let hue = 120.0 * t; // red -> yellow -> green
    let saturation = 0.85;
    let lightness = 0.45 + 0.15 * t;
    Color::hsl(hue, saturation, lightness)
}

/// Wrap an angle into `(-pi, pi]`.
fn wrap_pi(angle: f32) -> f32 {
    let a = angle.rem_euclid(std::f32::consts::TAU);
    if a > std::f32::consts::PI {
        a - std::f32::consts::TAU
    } else {
        a
    }
}

/// Smoothly interpolate the sprite from its previous cell to its current one
/// over a single step, giving the shell its roll and the core its eased turn.
/// On top of that, offset it toward the solid it hugs so it looks like it is
/// resting on the surface.
pub fn update_ball_transform(
    run: Res<Run>,
    tuning: Res<Tuning>,
    grid: Res<Grid>,
    mut balls: Query<(&Ball, &mut Transform, &Children)>,
    mut shells: Query<&mut Transform, (With<BallShell>, Without<Ball>, Without<BallCore>)>,
    mut cores: Query<&mut Transform, (With<BallCore>, Without<Ball>, Without<BallShell>)>,
) {
    let interval = 1.0 / (STEPS_PER_SECOND * run.speed.max(0.01));

    for (ball, mut transform, children) in &mut balls {
        // Paused, manual or finished runs sit at the logical cell. Otherwise
        // the ball slides from `prev` to `cell` over the step's interval.
        let progress = if run.outcome != Outcome::Running || tuning.manual {
            1.0
        } else {
            (ball.timer / interval).clamp(0.0, 1.0)
        };

        let from = grid.cell_to_world(ball.prev);
        let to = grid.cell_to_world(ball.cell);
        let center = from.lerp(to, progress);

        // Sit against the surface: blend the normal across the move and close
        // the gap between the ball's edge and the solid.
        let normal = surface_normal(&grid, ball.prev)
            .lerp(surface_normal(&grid, ball.cell), progress);
        let normal = if normal.length_squared() > f32::EPSILON {
            normal.normalize()
        } else {
            Vec2::new(0.0, -1.0)
        };
        let gap = (CELL_PX * 0.5 - BALL_RADIUS).max(0.0);
        let pos = center + normal * gap;

        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
        // Only the shell rolls. The core eases toward the surface normal (deck
        // away from the ground); its batteries ride along.
        let roll = ball.spin + progress * ball.spin_delta;
        let core_angle = ball.core_angle + progress * ball.core_delta;
        for child in children.iter() {
            if let Ok(mut shell) = shells.get_mut(child) {
                shell.rotation = Quat::from_rotation_z(roll);
            } else if let Ok(mut core) = cores.get_mut(child) {
                core.rotation = Quat::from_rotation_z(core_angle);
            }
        }
    }
}

/// In manual mode, one step per press of `N`.
pub fn manual_step(
    keys: Res<ButtonInput<KeyCode>>,
    tuning: Res<Tuning>,
    grid: Res<Grid>,
    mut run: ResMut<Run>,
    mut balls: Query<&mut Ball>,
) {
    // The `Tab` that enters run mode only activates manual control; the next
    // press nudges the ball.
    if run.just_entered {
        run.just_entered = false;
        return;
    }
    // `Tab` always nudges the ball one cell. `N` does the same, but only in
    // the debug manual mode where automatic stepping is already paused.
    let nudge = keys.just_pressed(KeyCode::Tab)
        || (tuning.manual && keys.just_pressed(KeyCode::KeyN));
    if !nudge || run.outcome != Outcome::Running {
        return;
    }
    for mut ball in &mut balls {
        step_once(&grid, &mut run, &mut ball);
    }
}

/// The generated arrow sprite used for the itinerary.
#[derive(Resource)]
pub struct ArrowTexture(pub Handle<Image>);

/// Marker for one itinerary arrow sprite.
#[derive(Component)]
pub struct ItineraryArrow;

/// Z of the itinerary arrows: above the grid, below the ball, so the ball is
/// never hidden behind its own path.
const ARROW_Z: f32 = 2.0;

/// Generate the arrow texture once at startup.
pub fn setup_arrow_texture(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(ArrowTexture(images.add(arrow_image())));
}

/// A white arrow pointing up (`+y`), tinted per move by the sprite colour.
fn arrow_image() -> Image {
    const SIZE: u32 = 24;
    let mut data = vec![0u8; (SIZE * SIZE * 4) as usize];
    let cx = SIZE as f32 * 0.5;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = (x as f32 + 0.5 - cx).abs();
            let fy = y as f32 + 0.5;
            // Head: a triangle from the tip down to y=14.
            let head = fy <= 14.0 && dx <= (fy / 14.0) * 8.0;
            // Shaft below the head.
            let shaft = fy >= 12.0 && dx <= 3.0;
            if head || shaft {
                let i = ((y * SIZE + x) * 4) as usize;
                data[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

/// Draw the movement itinerary: an arrow at every cell the ball left, coloured
/// purely by what the step did to the charge — green accumulates, orange
/// consumes. The arrows are sprites at [`ARROW_Z`], so the ball draws over them.
pub fn draw_itinerary(
    mut commands: Commands,
    run: Res<Run>,
    grid: Res<Grid>,
    arrows: Res<ArrowTexture>,
    mut state: Local<(u64, u64, VecDeque<Entity>)>,
) {
    let (front, next, spawned) = &mut *state;

    // A fresh run (or a cleared itinerary) drops every arrow.
    if run.total_moves == 0 {
        for entity in spawned.drain(..) {
            commands.entity(entity).despawn();
        }
        *front = 0;
        *next = 0;
        return;
    }

    // Moves that fell off the front of the trimmed itinerary lose their arrow.
    let base = run.total_moves - run.itinerary.len() as u64;
    while *front < base {
        if let Some(entity) = spawned.pop_front() {
            commands.entity(entity).despawn();
        }
        *front += 1;
    }

    // Spawn an arrow for every move recorded since last frame.
    while *next < run.total_moves {
        let m = run.itinerary[(*next - base) as usize];
        let dir = m.dir.as_vec2();
        let start = grid.cell_to_world(m.cell);
        let color = if m.charge > 0.0 {
            Color::srgb(0.30, 0.85, 0.35)
        } else {
            Color::srgb(1.0, 0.55, 0.10)
        };
        let entity = commands
            .spawn((
                ItineraryArrow,
                Sprite {
                    image: arrows.0.clone(),
                    color,
                    custom_size: Some(Vec2::new(CELL_PX * 0.6, CELL_PX * 0.9)),
                    ..default()
                },
                Transform::from_xyz(
                    start.x + dir.x * CELL_PX * 0.45,
                    start.y + dir.y * CELL_PX * 0.45,
                    ARROW_Z,
                )
                .with_rotation(Quat::from_rotation_z(
                    dir.to_angle() - std::f32::consts::FRAC_PI_2,
                )),
            ))
            .id();
        spawned.push_back(entity);
        *next += 1;
    }
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

/// Rotate an 8-neighbour offset 45 degrees clockwise (world +y is up), so a
/// ball stepping through the ring walks clockwise around its anchor.
fn cw45(offset: IVec2) -> IVec2 {
    match (offset.x, offset.y) {
        (1, 0) => IVec2::new(1, -1),
        (1, -1) => IVec2::new(0, -1),
        (0, -1) => IVec2::new(-1, -1),
        (-1, -1) => IVec2::new(-1, 0),
        (-1, 0) => IVec2::new(-1, 1),
        (-1, 1) => IVec2::new(0, 1),
        (0, 1) => IVec2::new(1, 1),
        (1, 1) => IVec2::new(1, 0),
        _ => offset,
    }
}

/// The solid stick point the ball starts on. Prefers the adjacent solid whose
/// clockwise step is exactly the anchor-derived heading, so the ball sets off
/// clockwise; falls back to any adjacent solid of the same mass.
fn initial_anchor(run: &Run, cell: IVec2) -> Option<IVec2> {
    let component = NEIGHBORS8
        .iter()
        .find_map(|offset| run.components.get(&(cell + *offset)).copied())?;
    let heading = initial_heading(run, component, cell);
    let preferred = NEIGHBORS8.iter().find_map(|offset| {
        let anchor = cell + *offset;
        (run.components.get(&anchor) == Some(&component)
            && cw45(cell - anchor) - (cell - anchor) == heading)
            .then_some(anchor)
    });
    preferred.or_else(|| {
        NEIGHBORS8
            .iter()
            .map(|offset| cell + *offset)
            .find(|anchor| run.components.get(anchor) == Some(&component))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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

    /// Paint a horizontal wall and return the top-surface start at its left end.
    fn wall(grid: &mut Grid, from: i32, to: i32, y: i32) -> IVec2 {
        for x in from..=to {
            grid.paint(IVec2::new(x, y), Cell::Solid);
        }
        IVec2::new(from, y + 1)
    }

    /// Run until the outcome stops being `Running` or `limit` steps elapse.
    fn run_for(grid: &mut Grid, run: &mut Run, ball: &mut Ball, limit: usize) {
        for _ in 0..limit {
            if run.outcome != Outcome::Running {
                break;
            }
            step_once(grid, run, ball);
        }
    }

    /// Every step the ball is adjacent to its anchor — it can never float off
    /// onto a different contour.
    #[test]
    fn ball_stays_on_its_anchor() {
        let mut grid = grid();
        let start = disk(&mut grid, 12.0);
        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(start, TEST_CHARGE);
        let mut visited = HashSet::new();

        for _ in 0..120 {
            if run.outcome != Outcome::Running {
                break;
            }
            step_once(&mut grid, &mut run, &mut ball);
            let anchor = ball.anchor.expect("anchor resolved");
            assert!(
                NEIGHBORS8.iter().any(|d| ball.cell + *d == anchor),
                "ball at {:?} left its anchor {anchor:?}",
                ball.cell
            );
            visited.insert(ball.cell);
        }
        assert!(visited.len() > 10, "ball barely moved: {}", visited.len());
    }

    /// Two parallel lines one cell apart: the ball stays anchored to the line
    /// it started on and never hops to the other.
    #[test]
    fn parallel_lines_do_not_hop() {
        let mut grid = grid();
        for y in 10..=20 {
            grid.paint(IVec2::new(0, y), Cell::Solid);
            grid.paint(IVec2::new(2, y), Cell::Solid);
        }

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(IVec2::new(1, 20), TEST_CHARGE);
        step_once(&mut grid, &mut run, &mut ball);
        let line = run.components[&ball.anchor.unwrap()];

        for _ in 0..60 {
            if run.outcome != Outcome::Running {
                break;
            }
            step_once(&mut grid, &mut run, &mut ball);
            assert_eq!(
                run.components[&ball.anchor.unwrap()],
                line,
                "hopped to the other line at {:?}",
                ball.cell
            );
        }
    }

    /// A wall end is rounded without a paused step: the anchor pivots around the
    /// corner while the ball keeps moving.
    #[test]
    fn anchor_rounds_the_corner() {
        let mut grid = grid();
        let start = wall(&mut grid, 5, 9, 5); // top surface at y=6, x=5..9
        assert_eq!(start, IVec2::new(5, 6));

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(start, TEST_CHARGE);

        // Five eastward moves put the ball past the end of the wall.
        for _ in 0..5 {
            step_once(&mut grid, &mut run, &mut ball);
        }
        assert_eq!(ball.cell, IVec2::new(10, 6));

        // The next move pivots around the end; it must not stall or reverse.
        step_once(&mut grid, &mut run, &mut ball);
        assert_eq!(run.outcome, Outcome::Running);
        assert!(
            ball.cell.x >= 10,
            "should pivot around the end, not bounce back: {:?}",
            ball.cell
        );
    }

    /// A horizontal move takes the sign of the vertical before it: a flat after
    /// a descent gains, a flat after a climb costs.
    #[test]
    fn horizontal_takes_preceding_sign() {
        // A single solid cell makes the ball ring it, alternating vertical runs
        // with horizontals.
        let mut grid = grid();
        grid.paint(IVec2::new(5, 5), Cell::Solid);

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(IVec2::new(5, 6), TEST_CHARGE);
        run_for(&mut grid, &mut run, &mut ball, 24);

        assert!(
            run.itinerary.iter().any(|m| m.combo && m.charge > 0.0),
            "a flat after a descent should gain"
        );
        assert!(
            run.itinerary.iter().any(|m| m.combo && m.charge < 0.0),
            "a flat after a climb should cost"
        );
    }

    /// Flat ground (horizontal with no preceding vertical) still consumes.
    #[test]
    fn flat_ground_costs() {
        let mut grid = grid();
        let start = wall(&mut grid, 5, 9, 5);

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(start, TEST_CHARGE);

        step_once(&mut grid, &mut run, &mut ball);
        assert!(!run.itinerary.last().unwrap().combo);
        assert_eq!(ball.charge, TEST_CHARGE - 1.0);
    }

    /// A ball with an empty battery cannot pay for a level move, so it explodes
    /// on the spot instead of taking the step.
    #[test]
    fn empty_battery_explodes_instead_of_moving() {
        let mut grid = grid();
        let start = wall(&mut grid, 5, 9, 5);

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(start, 0.0);

        step_once(&mut grid, &mut run, &mut ball);

        assert_eq!(run.outcome, Outcome::Depleted);
        assert_eq!(ball.cell, start, "ball moved without paying");
        assert!(
            run.itinerary.is_empty(),
            "a failed move must not be recorded"
        );
    }

    /// Rolling downhill still gains charge, so an empty battery may take that
    /// step — the explosion is only for moves the battery cannot afford.
    #[test]
    fn empty_battery_can_roll_downhill() {
        let mut grid = grid();
        grid.paint(IVec2::new(5, 5), Cell::Solid);

        let mut run = Run::default();
        start_run(&mut run, &grid);
        // Start east of the solid; the clockwise step is south, downhill.
        let mut ball = Ball::new(IVec2::new(6, 5), 0.0);

        step_once(&mut grid, &mut run, &mut ball);

        assert_eq!(run.outcome, Outcome::Running);
        assert_eq!(ball.cell, IVec2::new(6, 4));
        assert_eq!(ball.charge, 1.0);
    }

    /// A charge-losing ring completes laps but never meets the guarantee, so it
    /// runs on until the battery empties.
    #[test]
    fn doomed_loop_runs_until_it_explodes() {
        let mut grid = grid();
        for y in 5..8 {
            for x in 5..8 {
                grid.paint(IVec2::new(x, y), Cell::Solid);
            }
        }
        let start = grid.find_start().unwrap();

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(start, TEST_CHARGE);
        run_for(&mut grid, &mut run, &mut ball, 500);

        assert!(!run.solved, "a charge-losing ring cannot be solved");
        assert!(run.laps >= 1, "the ball still completed a lap first");
        assert_eq!(run.outcome, Outcome::Depleted);
    }

    /// A loop that meets its charge guarantee is marked solved, but the ball
    /// keeps looping (and getting faster) instead of ending the run.
    #[test]
    fn solved_loop_keeps_running() {
        let mut grid = grid();
        for x in 10..=13 {
            for y in 10..=18 {
                if x == 10 || x == 13 || y == 10 || y == 18 {
                    grid.paint(IVec2::new(x, y), Cell::Solid);
                }
            }
        }
        let start = IVec2::new(11, 17);

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(start, TEST_CHARGE);

        while run.outcome == Outcome::Running && run.laps < 3 {
            step_once(&mut grid, &mut run, &mut ball);
        }

        assert!(run.solved, "the box loop should solve");
        assert!(run.laps >= 3, "laps should keep being counted");
        assert!(run.speed > 1.0, "each lap should speed the ball up");
        assert_eq!(
            run.outcome,
            Outcome::Running,
            "a solved loop must keep running"
        );
    }

    /// An eternal loop eventually overloads and clears the level.
    #[test]
    fn solved_loop_eventually_wins() {
        let mut grid = grid();
        for x in 10..=13 {
            for y in 10..=18 {
                if x == 10 || x == 13 || y == 10 || y == 18 {
                    grid.paint(IVec2::new(x, y), Cell::Solid);
                }
            }
        }
        let start = IVec2::new(11, 17);

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(start, TEST_CHARGE);
        run_for(&mut grid, &mut run, &mut ball, 2000);

        assert!(run.solved, "it must solve before it can overload");
        assert_eq!(run.outcome, Outcome::Victory, "the loop should clear the level");
    }

    /// A tiny sealed pocket is a two-state cycle, so the `(cell, anchor)` loop
    /// condition closes it (the old cell-only `visits` never could).
    #[test]
    fn sealed_pocket_is_a_closed_loop() {
        // The debug_config diamond: a 2-cell vertical cavity in a solid ring.
        let mut grid = grid();
        for solid in [
            IVec2::new(77, 79),
            IVec2::new(76, 78),
            IVec2::new(78, 78),
            IVec2::new(76, 77),
            IVec2::new(78, 77),
            IVec2::new(77, 76),
        ] {
            grid.paint(solid, Cell::Solid);
        }
        let start = IVec2::new(77, 78);

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(start, 0.0);
        run_for(&mut grid, &mut run, &mut ball, 200);

        assert!(run.solved, "the pocket cycle should close");
        assert_eq!(run.outcome, Outcome::Victory);
    }

    /// A forward move is a full signed turn from the cell left to the cell
    /// entered. On a top surface, travelling right rolls clockwise.
    #[test]
    fn a_move_is_one_full_turn() {
        let mut grid = grid();
        let start = wall(&mut grid, 4, 8, 5);

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(start, TEST_CHARGE);

        step_once(&mut grid, &mut run, &mut ball);

        assert_eq!(ball.cell, IVec2::new(5, 6));
        assert_eq!(ball.prev, IVec2::new(4, 6), "the move remembers its start");
        assert!(
            ball.spin_delta < 0.0,
            "moving right on top should roll clockwise"
        );
        assert!(
            (ball.spin_delta.abs() - std::f32::consts::TAU * SHELL_ROLL_PER_CELL).abs() < 1e-4,
            "each cell should roll a fixed fraction of a turn"
        );
    }

    /// The inner core never snaps to a new normal: on each cell transition it
    /// turns by at most [`CORE_ALIGN_PER_CELL`].
    #[test]
    fn core_alignment_steps_gradually() {
        let mut grid = grid();
        for y in 5..9 {
            for x in 5..9 {
                grid.paint(IVec2::new(x, y), Cell::Solid);
            }
        }
        let start = grid.find_start().unwrap();

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(start, TEST_CHARGE);

        let mut moved_total = 0.0;
        for _ in 0..24 {
            if run.outcome != Outcome::Running {
                break;
            }
            let before = ball.core_angle;
            step_once(&mut grid, &mut run, &mut ball);
            let moved = wrap_pi(ball.core_angle - before).abs();
            assert!(
                moved <= CORE_ALIGN_PER_CELL + 1e-4,
                "core turned {moved} rad in one cell"
            );
            moved_total += moved;
        }
        assert!(moved_total > 0.0, "the core should turn as the ground bends");
    }

    /// A loop is all one direction: every move on a clockwise contour rolls
    /// the same way, so the ball does not judder back and forth.
    #[test]
    fn a_loop_rolls_consistently() {
        let mut grid = grid();
        for y in 5..8 {
            for x in 5..8 {
                grid.paint(IVec2::new(x, y), Cell::Solid);
            }
        }
        let start = grid.find_start().unwrap();

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(start, TEST_CHARGE);

        let mut signs = Vec::new();
        for _ in 0..20 {
            if run.outcome != Outcome::Running {
                break;
            }
            step_once(&mut grid, &mut run, &mut ball);
            signs.push(ball.spin_delta.signum());
        }

        assert!(signs.len() > 8, "the ball should have moved around the ring");
        let first = signs[0];
        assert!(
            signs.iter().all(|s| *s == first),
            "roll direction changed within a loop: {signs:?}"
        );
    }

    /// A ball walled in with no surface around its anchor is stuck.
    #[test]
    fn stuck_when_fully_enclosed() {
        let mut grid = grid();
        for offset in NEIGHBORS8 {
            grid.set(IVec2::new(10, 10) + offset, Cell::Solid);
        }

        let mut run = Run::default();
        start_run(&mut run, &grid);
        let mut ball = Ball::new(IVec2::new(10, 10), TEST_CHARGE);

        step_once(&mut grid, &mut run, &mut ball);
        assert_eq!(run.outcome, Outcome::Stuck);
    }
}
