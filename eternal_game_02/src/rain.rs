//! Cyberpunk backdrop and falling rain.
//!
//! The backdrop is one stretched gradient sprite behind the board. Rain is a
//! pool of real `Rain` entities: teardrop sprites that fall each frame. Each
//! drop checks the grid cell ahead of it and, on hitting a solid, is destroyed
//! and replaced at the top. The impact leaves a splash that morphs ball ->
//! ellipse -> flat puddle while a few droplets fly outwards.
//!
//! Collision is a single matrix lookup per drop (`world_to_cell` then
//! `Grid::get`), so the whole system is O(drops).
//!
//! Everything is hidden in debug/editor mode, leaving the black clear colour.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::grid::{CELL_PX, Cell, Grid};
use crate::paint::Debug;

/// The static skybox behind the board.
#[derive(Component)]
pub struct CyberBackground;

/// The droplet and splat sprites, loaded once.
#[derive(Resource)]
pub struct RainAssets {
    drop: Handle<Image>,
    splat: Handle<Image>,
}

/// A falling rain droplet.
#[derive(Component)]
pub struct Rain {
    speed: f32,
    length: f32,
    phase: f32,
}

/// The squashing mark left by an impact: ball, then ellipse, then flat.
#[derive(Component)]
pub struct Impact {
    life: f32,
    max_life: f32,
}

/// A droplet thrown outwards by an impact.
#[derive(Component)]
pub struct Splash {
    life: f32,
    max_life: f32,
    velocity: Vec2,
    spin: f32,
}

/// How many droplets are kept in the air.
const RAIN_COUNT: usize = 24;
const BG_Z: f32 = -20.0;
const RAIN_Z: f32 = -15.0;
const IMPACT_Z: f32 = 1.0;
const SPLASH_Z: f32 = 1.5;
/// Width / height of `rain_drop.png`.
const DROP_ASPECT: f32 = 16.0 / 24.0;

/// Tiny deterministic RNG, so the game does not need a `rand` dependency.
pub struct Rng(u32);

impl Default for Rng {
    fn default() -> Self {
        Self(0x9E37_79B9)
    }
}

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        ((self.0 >> 8) & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

/// Spawn the backdrop once. It is huge and centred, so it covers the board and
/// a good margin of panning.
pub fn setup_background(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        CyberBackground,
        Sprite {
            image: images.add(background_image()),
            custom_size: Some(Vec2::new(2200.0, 1600.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, BG_Z),
    ));
}

/// Load the droplet and splat textures once, nearest-filtered.
pub fn setup_rain_assets(mut commands: Commands, assets: Res<AssetServer>) {
    let nearest = |path: &'static str| -> Handle<Image> {
        assets
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| {
                settings.sampler = ImageSampler::nearest();
            })
            .load(path)
    };
    commands.insert_resource(RainAssets {
        drop: nearest("sprites/rain_drop.png"),
        splat: nearest("sprites/splat.png"),
    });
}

/// Fill the air with droplets, spread through the whole column so it starts
/// looking like rain immediately.
pub fn setup_rain(mut commands: Commands, grid: Res<Grid>, assets: Res<RainAssets>) {
    let half = grid.world_size() * 0.5;
    let mut rng = Rng::default();
    for _ in 0..RAIN_COUNT {
        spawn_drop(&mut commands, &mut rng, half, &assets, false);
    }
}

/// One teardrop, ready to fall.
fn spawn_drop(
    commands: &mut Commands,
    rng: &mut Rng,
    half: Vec2,
    assets: &RainAssets,
    from_top: bool,
) {
    let x = rng.range(-half.x - 60.0, half.x + 60.0);
    let y = if from_top {
        half.y + rng.range(0.0, 400.0)
    } else {
        rng.range(-half.y, half.y)
    };
    let length = rng.range(9.0, 18.0);
    let alpha = rng.range(0.22, 0.5);
    commands.spawn((
        Rain {
            speed: rng.range(240.0, 520.0),
            length,
            phase: rng.range(0.0, std::f32::consts::TAU),
        },
        Sprite {
            image: assets.drop.clone(),
            color: Color::srgba(0.55, 0.80, 1.0, alpha),
            custom_size: Some(Vec2::new(length * DROP_ASPECT, length)),
            ..default()
        },
        Transform::from_xyz(x, y, RAIN_Z),
    ));
}

/// Move every droplet down, check what its leading tip is about to touch, and
/// either keep falling or annihilate against a solid.
pub fn update_rain(
    time: Res<Time>,
    grid: Res<Grid>,
    debug: Res<Debug>,
    assets: Res<RainAssets>,
    mut commands: Commands,
    mut rng: Local<Rng>,
    mut drops: Query<(Entity, &mut Transform, &mut Sprite, &mut Rain)>,
) {
    if debug.0 {
        return;
    }
    let dt = time.delta_secs();
    let half = grid.world_size() * 0.5;

    for (entity, mut transform, mut sprite, mut drop) in &mut drops {
        transform.translation.y -= drop.speed * dt;

        // A subtle stretch/squash so the fall reads as animated.
        drop.phase += dt * 9.0;
        let wobble = 1.0 + 0.05 * drop.phase.sin();
        sprite.custom_size = Some(Vec2::new(
            drop.length * DROP_ASPECT * wobble,
            drop.length / wobble,
        ));

        // The leading edge is the lower end of the teardrop. Sweep it from
        // where it was last frame to where it is now, so a fast drop (or a big
        // frame step) cannot tunnel through a block, and test a short mask line
        // across its width at every sample.
        let tip = Vec2::new(
            transform.translation.x,
            transform.translation.y - drop.length * 0.5,
        );
        let previous_tip = Vec2::new(tip.x, tip.y + drop.speed * dt);
        let mask = drop.length * DROP_ASPECT * 0.25;
        let hit = sweep_hit(&grid, previous_tip, tip, mask);

        if let Some(cell) = hit {
            // Impact on the top face of the solid it struck.
            let world = grid.cell_to_world(cell);
            let at = Vec2::new(world.x, world.y + CELL_PX * 0.5);
            spawn_impact(&mut commands, &assets, at);
            spawn_splash(&mut commands, &mut rng, &assets, at);
        } else if transform.translation.y >= -half.y - 80.0 {
            continue; // still falling
        }

        // Gone: annihilated on impact, or fell past the bottom. Replace it.
        commands.entity(entity).despawn();
        spawn_drop(&mut commands, &mut rng, half, &assets, true);
    }
}

/// The first solid cell touched by the leading edge as it travels from `from`
/// to `to`. The edge is sampled every fraction of a cell along the travel, and
/// at each sample a short horizontal mask line (centre and both edges) is
/// tested, so a drop cannot slip past a block between frames.
fn sweep_hit(grid: &Grid, from: Vec2, to: Vec2, mask: f32) -> Option<IVec2> {
    let distance = (to - from).length();
    let steps = (distance / (CELL_PX * 0.4)).ceil().max(1.0) as i32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let point = from.lerp(to, t);
        for dx in [-mask, 0.0, mask] {
            let sample = Vec2::new(point.x + dx, point.y);
            if let Some(cell) = grid.world_to_cell(sample)
                && grid.get(cell) == Some(Cell::Solid)
            {
                return Some(cell);
            }
        }
    }
    None
}

/// The flattening mark left at an impact point.
fn spawn_impact(commands: &mut Commands, assets: &RainAssets, at: Vec2) {
    commands.spawn((
        Impact {
            life: 0.34,
            max_life: 0.34,
        },
        Sprite {
            image: assets.splat.clone(),
            color: Color::srgba(0.70, 0.90, 1.0, 0.95),
            custom_size: Some(Vec2::splat(CELL_PX * 0.8)),
            ..default()
        },
        Transform::from_xyz(at.x, at.y, IMPACT_Z),
    ));
}

/// Animate the impact: the ball widens into an ellipse, flattens, and fades.
pub fn update_impact(
    time: Res<Time>,
    mut commands: Commands,
    mut impacts: Query<(Entity, &mut Transform, &mut Sprite, &mut Impact)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut sprite, mut impact) in &mut impacts {
        impact.life -= dt;
        if impact.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        // t runs 0 -> 1 over the life; easing makes the initial spread snap and
        // the final flatten settle.
        let t = 1.0 - (impact.life / impact.max_life).clamp(0.0, 1.0);
        let ease = t.sqrt();
        let width = 0.85 + 1.9 * ease; // ball -> wide ellipse
        let height = 0.85 - 0.63 * ease; // ellipse -> flat puddle
        sprite.custom_size = Some(Vec2::new(CELL_PX * 0.8 * width, CELL_PX * 0.8 * height));
        sprite.color.set_alpha(0.95 * (1.0 - t));
        transform.translation.y -= dt * 5.0 * ease; // settle as it flattens
    }
}

/// A few small droplets thrown outwards from the impact.
fn spawn_splash(commands: &mut Commands, rng: &mut Rng, assets: &RainAssets, at: Vec2) {
    for i in 0..4 {
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let size = rng.range(2.6, 4.2);
        commands.spawn((
            Splash {
                life: 0.34,
                max_life: 0.34,
                velocity: Vec2::new(side * rng.range(20.0, 75.0), rng.range(30.0, 95.0)),
                spin: side * rng.range(4.0, 10.0),
            },
            Sprite {
                image: assets.drop.clone(),
                color: Color::srgba(0.65, 0.90, 1.0, 0.95),
                custom_size: Some(Vec2::new(size * DROP_ASPECT, size)),
                ..default()
            },
            Transform::from_xyz(at.x, at.y, SPLASH_Z),
        ));
    }
}

/// Fly the outwards droplets, then fade them out.
pub fn update_splash(
    time: Res<Time>,
    mut commands: Commands,
    mut splashes: Query<(Entity, &mut Transform, &mut Sprite, &mut Splash)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut sprite, mut splash) in &mut splashes {
        splash.life -= dt;
        if splash.life <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        splash.velocity.y -= 180.0 * dt;
        transform.translation.x += splash.velocity.x * dt;
        transform.translation.y += splash.velocity.y * dt;
        transform.rotate_z(splash.spin * dt);

        let t = (splash.life / splash.max_life).clamp(0.0, 1.0);
        sprite.color.set_alpha(0.95 * t);
    }
}

/// Hide the backdrop and all rain while debugging, so the board sits on plain
/// black.
pub fn apply_atmosphere_visibility(
    debug: Res<Debug>,
    mut visible: Query<
        &mut Visibility,
        Or<(With<CyberBackground>, With<Rain>, With<Impact>, With<Splash>)>,
    >,
) {
    let target = if debug.0 {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };
    for mut visibility in &mut visible {
        *visibility = target;
    }
}

/// A 4x256 vertical cyberpunk gradient: dark indigo up top, a magenta glow
/// near the horizon, and dark again below.
fn background_image() -> Image {
    const W: u32 = 4;
    const H: u32 = 256;
    let mut data = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        let t = y as f32 / (H - 1) as f32;
        let (r, g, b) = sky_color(t);
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            data[i..i + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

fn sky_color(t: f32) -> (u8, u8, u8) {
    const STOPS: [(f32, (f32, f32, f32)); 5] = [
        (0.00, (6.0, 4.0, 20.0)),
        (0.34, (16.0, 8.0, 42.0)),
        (0.54, (52.0, 14.0, 72.0)),
        (0.62, (126.0, 34.0, 102.0)),
        (1.00, (8.0, 4.0, 18.0)),
    ];
    let mut lo = STOPS[0];
    let mut hi = STOPS[STOPS.len() - 1];
    for pair in STOPS.windows(2) {
        if t >= pair[0].0 && t <= pair[1].0 {
            lo = pair[0];
            hi = pair[1];
            break;
        }
    }
    let span = (hi.0 - lo.0).max(1e-4);
    let f = ((t - lo.0) / span).clamp(0.0, 1.0);
    let mix = |a: f32, b: f32| (a + (b - a) * f) as u8;
    (
        mix(lo.1.0, hi.1.0),
        mix(lo.1.1, hi.1.1),
        mix(lo.1.2, hi.1.2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drop that jumps clean over a block in one frame must still hit it.
    #[test]
    fn sweep_catches_a_solid_between_frames() {
        let mut grid = Grid::new(Handle::default());
        grid.set(IVec2::new(0, 0), Cell::Solid);
        let centre = grid.cell_to_world(IVec2::new(0, 0));
        let from = centre + Vec2::new(0.0, CELL_PX * 4.0);
        let to = centre - Vec2::new(0.0, CELL_PX * 4.0);
        assert_eq!(sweep_hit(&grid, from, to, 0.0), Some(IVec2::new(0, 0)));
    }

    /// A drop falling past the side of a block, mask and all, must miss it.
    #[test]
    fn sweep_misses_when_passing_beside_a_solid() {
        let mut grid = Grid::new(Handle::default());
        grid.set(IVec2::new(0, 0), Cell::Solid);
        let x = grid.cell_to_world(IVec2::new(0, 0)).x + CELL_PX * 3.0;
        let from = Vec2::new(x, CELL_PX * 10.0);
        let to = Vec2::new(x, -CELL_PX * 10.0);
        assert_eq!(sweep_hit(&grid, from, to, CELL_PX * 0.3), None);
    }
}
