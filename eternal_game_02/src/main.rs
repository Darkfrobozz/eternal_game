//! Eternal Game 02 — a puzzle game about drawing surfaces and building charge.
//!
//! Phase 1: the [`Grid`] array plus a mouse brush.
//! Phase 2: the pen grows surface around solids, and a ball walks the
//! flood-filled route clockwise while accumulating charge.

mod ball;
mod config;
mod explosion;
mod grid;
mod menu;
mod paint;
mod rain;
mod screen;
mod sfx;
mod tutorial;

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use ball::{
    Run, Tuning, draw_itinerary, manual_step, setup_arrow_texture, setup_ball_texture, step_ball,
    tune_start_charge, update_ball_color, update_ball_transform, update_charge_text,
};
use grid::{CELL_PX, GRID_H, GRID_W};
use config::{Levels, Progress, advance_detonation, cycle_level, setup_levels, update_level_text};
use explosion::{
    Detonation, setup_victory_text, show_victory, spawn_explosion, update_detonation,
    update_explosion, update_pending_bursts,
};
use menu::{Menu, Screen, draw_menu, menu_input, setup_menu};
use paint::{
    Brush, Debug, Mode, Placement, apply_hud, handle_mode, leave_run_on_death, paint, place_start,
    report_outcome, apply_solids, setup_grid, setup_solid_tile, sync_image, toggle_debug,
    watch_solid_tile,
};
use rain::{
    apply_atmosphere_visibility, setup_background, setup_rain, setup_rain_assets, update_impact,
    update_rain, update_splash,
};
use screen::position_screen_text;
use sfx::setup_sfx;
use tutorial::{
    Tutorial, apply_tutorial_visibility, draw_tutorial, setup_tutorial, track_tutorial,
};

fn main() {
    // Headless: `cargo run -- --replay [file] [steps]`.
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--replay") {
        let path = args
            .get(pos + 1)
            .map(String::as_str)
            .unwrap_or(config::CONFIG_PATH);
        let steps = args
            .get(pos + 2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(120);
        config::replay(path, steps);
        return;
    }

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                resolution: (
                    (GRID_W as f32 * CELL_PX) as u32,
                    (GRID_H as f32 * CELL_PX + 46.0) as u32,
                )
                    .into(),
                resizable: true,
                title: "Eternal Game 02".into(),
                ..default()
            }),
            ..default()
        }))
        .init_resource::<Brush>()
        .init_resource::<Mode>()
        .init_resource::<Placement>()
        .init_resource::<Run>()
        .init_resource::<Tuning>()
        .init_resource::<Debug>()
        .init_resource::<Levels>()
        .init_resource::<Progress>()
        .init_resource::<Tutorial>()
        .init_resource::<Menu>()
        .init_resource::<Screen>()
        .init_resource::<Detonation>()
        .insert_resource(ClearColor(Color::BLACK))
        .add_systems(
            Startup,
            (
                setup_grid,
                setup_solid_tile,
                setup_background,
                setup_rain_assets,
                setup_sfx,
                setup_levels,
                setup_camera,
                setup_tutorial,
                setup_ball_texture,
                setup_arrow_texture,
                setup_victory_text,
                setup_menu,
            )
                .chain(),
        )
        .add_systems(PostStartup, setup_rain)
        .add_systems(
            Update,
            (
                config::debug_io,
                cycle_level.run_if(in_play),
                tune_start_charge,
                place_start.run_if(in_play),
                handle_mode.run_if(in_play),
                paint.run_if(in_paint_mode),
                apply_solids,
                step_ball.run_if(in_run_mode),
                manual_step.run_if(in_run_mode),
                watch_solid_tile,
                sync_image,
                update_ball_transform,
                update_ball_color,
                draw_itinerary,
                update_charge_text,
                update_level_text,
                toggle_debug.run_if(in_play),
                apply_hud,
                report_outcome,
                camera_controls.run_if(in_play),
            )
                .chain(),
        )
        // The death burst reads the outcome the stepping systems just wrote,
        // so it runs after them (and its particles fade every frame).
        .add_systems(
            Update,
            (
                spawn_explosion,
                // Dying kicks the player back into edit mode. Must run after the
                // burst has consumed the ball.
                leave_run_on_death,
                update_detonation,
                advance_detonation,
                show_victory,
                update_explosion,
                update_pending_bursts,
            )
                .chain()
                .after(step_ball)
                .after(manual_step),
        )
        // The backdrop and rain are independent of the board simulation, so
        // they get their own set (and keep the main chain under the tuple
        // size limit).
        .add_systems(
            Update,
            (apply_atmosphere_visibility, update_rain, update_impact, update_splash).chain(),
        )
        // Screen-space labels and the tutorial read input and react to the
        // camera, so they run after it has been moved this frame.
        .add_systems(
            Update,
            (
                track_tutorial,
                draw_tutorial,
                apply_tutorial_visibility,
                menu_input,
                draw_menu,
                position_screen_text,
            )
                .chain()
                .after(camera_controls),
        )
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: DEFAULT_ZOOM,
            ..OrthographicProjection::default_2d()
        }),
    ));
}

/// Closest the camera can zoom in (smallest orthographic scale).
const MIN_ZOOM: f32 = 0.15;
/// Furthest the camera can pull back (largest orthographic scale).
const MAX_ZOOM: f32 = 2.0;
/// Default view: the middle of the allowed zoom range.
const DEFAULT_ZOOM: f32 = (MIN_ZOOM + MAX_ZOOM) / 2.0;

fn in_paint_mode(mode: Res<Mode>, screen: Res<Screen>) -> bool {
    *mode == Mode::Paint && *screen != Screen::Menu
}

fn in_run_mode(mode: Res<Mode>, screen: Res<Screen>) -> bool {
    *mode == Mode::Run && *screen != Screen::Menu
}

/// True while a game or the map editor is showing (i.e. not the title menu).
fn in_play(screen: Res<Screen>) -> bool {
    *screen != Screen::Menu
}

/// Scroll to zoom, WASD to pan. Keeps the full grid available while letting you
/// zoom in far enough to read the itinerary arrows.
fn camera_controls(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut wheel: MessageReader<MouseWheel>,
    mut camera: Query<(&mut Transform, &mut Projection, &Camera), With<Camera2d>>,
) {
    let mut zoom = 0.0;
    for event in wheel.read() {
        zoom += event.y;
    }
    let pan_speed = 600.0 * time.delta_secs();
    let world_half = Vec2::new(GRID_W as f32, GRID_H as f32) * CELL_PX * 0.5;

    for (mut transform, mut projection, camera) in &mut camera {
        let mut scale = 1.0;
        if let Projection::Orthographic(ortho) = &mut *projection {
            if zoom != 0.0 {
                ortho.scale = (ortho.scale * 0.9_f32.powf(zoom)).clamp(MIN_ZOOM, MAX_ZOOM);
            }
            scale = ortho.scale;
        }
        let mut pan = Vec2::ZERO;
        if keys.pressed(KeyCode::KeyW) {
            pan.y += 1.0;
        }
        if keys.pressed(KeyCode::KeyS) {
            pan.y -= 1.0;
        }
        if keys.pressed(KeyCode::KeyA) {
            pan.x -= 1.0;
        }
        if keys.pressed(KeyCode::KeyD) {
            pan.x += 1.0;
        }
        if pan != Vec2::ZERO {
            transform.translation += (pan.normalize() * pan_speed * scale).extend(0.0);
        }

        // Keep the board filling the view: never show anything beyond the
        // world. If the view is bigger than the world on an axis, the limit
        // collapses to zero and the camera just centres on that axis.
        if let Some(viewport) = camera.logical_viewport_size() {
            let half_view = viewport * scale * 0.5;
            let limit = (world_half - half_view).max(Vec2::ZERO);
            transform.translation.x = transform.translation.x.clamp(-limit.x, limit.x);
            transform.translation.y = transform.translation.y.clamp(-limit.y, limit.y);
        }
    }
}
