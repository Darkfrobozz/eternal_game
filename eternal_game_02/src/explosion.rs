//! The ball's death burst — and the level-clearing overload.
//!
//! A run ends one of three ways:
//!
//! * [`Outcome::Depleted`] — the battery was empty and the ball tried to move,
//! * [`Outcome::Stuck`] — there was nowhere left to go,
//! * [`Outcome::Victory`] — a self-sustaining loop ran long enough to overload.
//!
//! The first two blow up just the ball. Victory blows up the **whole level**: a
//! shockwave from the ball plus a chain reaction of bursts over every solid and
//! track cell, followed by a "level cleared" banner.
//!
//! The bursts are drawn with plain sprites (the project avoids `bevy_ui` and
//! assets), and are deterministic rather than random so the effect is the same
//! every time and stays cheap to reason about.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::ball::{Ball, Outcome, Run};
use crate::grid::{CELL_PX, Cell, Grid};
use crate::screen::{ScreenAnchor, ScreenText};
use crate::sfx::{self, Sfx};

/// World z of the burst, above the ball (5) and the grid (0).
const BURST_Z: f32 = 8.0;
/// Most bursts the victory chain reaction will spawn across the level. Larger
/// levels are sampled down to this, so the effect stays affordable.
const MAX_LEVEL_BURSTS: usize = 220;
/// How fast the victory shockwave sweeps across the board, in world units per
/// second. A new burst lands roughly every `CELL_PX / SPEED` seconds.
const LEVEL_BLAST_SPEED: f32 = 2200.0;
/// Seconds the victory dissolve takes to burn the level to ash and clear it.
pub const DETONATION_TIME: f32 = 1.6;

/// Drives the victory sequence: the level dissolves to ash, then either the
/// next level loads or the final VICTORY banner appears.
#[derive(Resource, Default, PartialEq, Debug)]
pub enum Detonation {
    #[default]
    Idle,
    /// The level is burning away; `elapsed` counts up to [`DETONATION_TIME`].
    Blasting { elapsed: f32 },
    /// The level is gone. If it was the last one, the VICTORY banner is up.
    Complete,
}

/// One piece of a burst (a flash or a spark).
///
/// [`update_explosion`] expands/moves and fades it each frame, then despawns it
/// once [`ExplosionParticle::life`] runs out.
#[derive(Component)]
pub struct ExplosionParticle {
    /// World units per second, damped by [`ExplosionParticle::drag`].
    velocity: Vec2,
    /// Seconds of life remaining.
    life: f32,
    /// Seconds this particle started with, for the `0..1` progress.
    max_life: f32,
    /// Sprite edge length at birth and at death (world units).
    start_size: f32,
    end_size: f32,
    color: Color,
    /// Per-second velocity damping. `0` keeps the flash centred.
    drag: f32,
}

/// A burst waiting its turn in the victory chain reaction.
#[derive(Component)]
pub struct PendingBurst {
    position: Vec2,
    delay: f32,
}

/// Marks the "level cleared" banner.
#[derive(Component)]
pub struct VictoryText;

/// Spawn the (initially hidden) victory banner.
pub fn setup_victory_text(mut commands: Commands) {
    commands.spawn((
        Text2d::new(""),
        TextFont {
            font_size: FontSize::Px(38.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.92, 0.45)),
        TextLayout::justify(Justify::Center),
        Anchor::CENTER,
        ScreenText {
            anchor: ScreenAnchor::Center,
            margin: 0.0,
            z: 50.0,
        },
        Transform::from_xyz(0.0, 0.0, 50.0),
        VictoryText,
        Visibility::Hidden,
    ));
}

/// Show the final VICTORY banner once the last level has been consumed and
/// there is nowhere left to go. (With a next level, `advance_detonation` loads
/// it instead, so no banner appears for it.)
pub fn show_victory(
    run: Res<Run>,
    detonation: Res<Detonation>,
    mut texts: Query<(&mut Text2d, &mut Visibility), With<VictoryText>>,
) {
    let visible = *detonation == Detonation::Complete && run.outcome == Outcome::Victory;
    for (mut text, mut visibility) in &mut texts {
        if visible {
            *visibility = Visibility::Visible;
            text.0 = "VICTORY".into();
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}

/// Spawn the explosion the first frame a run ends. A `Local` guard makes it
/// fire exactly once per run; it re-arms when the next run starts.
pub fn spawn_explosion(
    run: Res<Run>,
    sfx: Res<Sfx>,
    mut grid: ResMut<Grid>,
    mut commands: Commands,
    balls: Query<(Entity, &Ball)>,
    mut fired: Local<bool>,
) {
    if !matches!(
        run.outcome,
        Outcome::Depleted | Outcome::Stuck | Outcome::Victory
    ) {
        *fired = false;
        return;
    }
    if *fired {
        return;
    }
    *fired = true;

    let Some((entity, ball)) = balls.iter().next() else {
        return;
    };
    let center = grid.cell_to_world(ball.cell);
    // The ball is consumed by the blast either way.
    commands.entity(entity).despawn();

    if run.outcome == Outcome::Victory {
        // The dissolve starts at the ball and spreads over the level's extent,
        // so the blast matches the level the player has zoomed to.
        let radius = level_radius(&grid);
        grid.dissolve = 0.0;
        grid.dissolve_origin = ball.cell;
        grid.dissolve_radius = radius;
        spawn_level_explosion(&mut commands, &grid, center);
        sfx::play(&mut commands, &sfx.victory);
    } else {
        spawn_burst(&mut commands, center, 16);
        sfx::play(&mut commands, &sfx.death);
    }
}

/// Advance the victory dissolve each frame. Once complete the whole board is
/// wiped clean; `advance_detonation` then loads the next level or leaves the
/// VICTORY banner up.
pub fn update_detonation(
    time: Res<Time>,
    run: Res<Run>,
    mut grid: ResMut<Grid>,
    mut detonation: ResMut<Detonation>,
) {
    // Any reset (menu, new level, leaving a run) aborts a sequence in flight.
    if run.outcome != Outcome::Victory {
        if *detonation != Detonation::Idle {
            *detonation = Detonation::Idle;
            grid.dissolve = 0.0;
            grid.dirty = true;
        }
        return;
    }

    match *detonation {
        Detonation::Idle => *detonation = Detonation::Blasting { elapsed: 0.0 },
        Detonation::Blasting { elapsed } => {
            let elapsed = elapsed + time.delta_secs();
            let progress = (elapsed / DETONATION_TIME).clamp(0.0, 1.0);
            grid.dissolve = progress;
            grid.dirty = true;
            if progress >= 1.0 {
                grid.obliterate();
                *detonation = Detonation::Complete;
            } else {
                *detonation = Detonation::Blasting { elapsed };
            }
        }
        Detonation::Complete => {}
    }
}

/// Half-diagonal of the level's bounding box, in cells, used to normalise the
/// dissolve wave. At least 1 so the division is safe.
fn level_radius(grid: &Grid) -> f32 {
    match grid.content_bounds() {
        Some((min, max)) => {
            let span = (max - min + IVec2::ONE).as_vec2();
            (span.length() * 0.5).max(1.0)
        }
        None => 1.0,
    }
}

/// The victory blast: a shockwave from the ball sized to the level, plus a
/// chain reaction that sweeps out over every cell the level occupies.
fn spawn_level_explosion(commands: &mut Commands, grid: &Grid, center: Vec2) {
    // Shockwave: a bright square that balloons to cover the level.
    let life = 0.75;
    commands.spawn((
        ExplosionParticle {
            velocity: Vec2::ZERO,
            life,
            max_life: life,
            start_size: CELL_PX,
            end_size: level_radius(grid) * CELL_PX * 2.6,
            color: Color::srgb(1.0, 0.97, 0.80),
            drag: 0.0,
        },
        Sprite::from_color(Color::srgb(1.0, 0.97, 0.80), Vec2::splat(CELL_PX)),
        Transform::from_xyz(center.x, center.y, BURST_Z + 1.0),
    ));

    // Every non-empty cell (walls and surface) becomes a burst. Large
    // levels are sampled down so the particle count stays bounded.
    let cells: Vec<IVec2> = (0..grid.h)
        .flat_map(|y| (0..grid.w).map(move |x| IVec2::new(x, y)))
        .filter(|cell| grid.get(*cell) != Some(Cell::Empty))
        .collect();
    let stride = (cells.len() / MAX_LEVEL_BURSTS).max(1);

    for cell in cells.into_iter().step_by(stride) {
        let position = grid.cell_to_world(cell);
        // The further from the ball, the later the cell detonates, so the
        // blast reads as a wave rolling across the level.
        let delay = position.distance(center) / LEVEL_BLAST_SPEED;
        commands.spawn(PendingBurst { position, delay });
    }
}

/// Detonate each queued burst once its delay has elapsed.
pub fn update_pending_bursts(
    time: Res<Time>,
    mut commands: Commands,
    mut pending: Query<(Entity, &mut PendingBurst)>,
) {
    let dt = time.delta_secs();
    for (entity, mut burst) in &mut pending {
        burst.delay -= dt;
        if burst.delay <= 0.0 {
            spawn_burst(&mut commands, burst.position, 4);
            commands.entity(entity).despawn();
        }
    }
}

/// Build one flash and `sparks` sparks at `center`.
fn spawn_burst(commands: &mut Commands, center: Vec2, sparks: usize) {
    // Central flash: a bright square that balloons and fades quickly.
    let flash_life = 0.30;
    commands.spawn((
        ExplosionParticle {
            velocity: Vec2::ZERO,
            life: flash_life,
            max_life: flash_life,
            start_size: CELL_PX * 0.5,
            end_size: CELL_PX * 2.4,
            color: Color::srgb(1.0, 0.96, 0.75),
            drag: 0.0,
        },
        Sprite::from_color(Color::srgb(1.0, 0.96, 0.75), Vec2::splat(CELL_PX * 0.5)),
        Transform::from_xyz(center.x, center.y, BURST_Z),
    ));

    // Sparks: a fan of warm slivers thrown outwards and dragged to a stop.
    for i in 0..sparks {
        // Evenly spaced around a circle, offset so the fan is not axis-aligned.
        let angle = (i as f32 / sparks as f32) * std::f32::consts::TAU + 0.4;
        let direction = Vec2::new(angle.cos(), angle.sin());
        let speed = 110.0 + 70.0 * ((i * 7 % 5) as f32);
        let life = 0.40 + 0.20 * ((i * 3 % 4) as f32);
        let color = match i % 3 {
            0 => Color::srgb(1.00, 0.88, 0.45),
            1 => Color::srgb(1.00, 0.55, 0.15),
            _ => Color::srgb(0.95, 0.28, 0.20),
        };
        let size = CELL_PX * 0.4;
        commands.spawn((
            ExplosionParticle {
                velocity: direction * speed,
                life,
                max_life: life,
                start_size: size,
                end_size: 0.0,
                color,
                drag: 3.5,
            },
            Sprite::from_color(color, Vec2::splat(size)),
            Transform::from_xyz(center.x, center.y, BURST_Z),
        ));
    }
}

/// Animate every live particle and despawn the ones that have burnt out.
pub fn update_explosion(
    time: Res<Time>,
    mut commands: Commands,
    mut particles: Query<(Entity, &mut Transform, &mut Sprite, &mut ExplosionParticle)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut sprite, mut particle) in &mut particles {
        particle.life -= dt;
        if particle.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Damp the velocity, then coast.
        let damping = (1.0 - particle.drag * dt).max(0.0);
        particle.velocity *= damping;
        transform.translation += (particle.velocity * dt).extend(0.0);

        let progress = 1.0 - particle.life / particle.max_life;
        let size =
            (particle.start_size + (particle.end_size - particle.start_size) * progress).max(0.0);
        sprite.custom_size = Some(Vec2::splat(size));
        sprite.color = particle.color.with_alpha(1.0 - progress);
    }
}
