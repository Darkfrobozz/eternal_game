//! Eternal Game 02 — a puzzle game about drawing surfaces and building charge.
//!
//! Phase 1: the [`Grid`] array plus a mouse brush.
//! Phase 2: the pen grows surface around solids, and a ball walks the
//! flood-filled route clockwise while accumulating charge.

mod ball;
mod config;
mod grid;
mod paint;

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use ball::{
    Run, Tuning, draw_itinerary, manual_step, step_ball, tune_start_charge, update_ball_color,
    update_ball_transform, update_charge_text,
};
use grid::{CELL_PX, GRID_H, GRID_W};
use paint::{
    Brush, Debug, Mode, Placement, apply_hud, handle_mode, paint, place_start, report_outcome,
    setup_grid, sync_image, toggle_debug,
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
        .add_systems(Startup, (setup_grid, setup_camera))
        .add_systems(
            Update,
            (
                config::debug_io,
                tune_start_charge,
                place_start,
                handle_mode,
                paint.run_if(in_paint_mode),
                step_ball.run_if(in_run_mode),
                manual_step.run_if(in_run_mode),
                sync_image,
                update_ball_transform,
                update_ball_color,
                draw_itinerary,
                update_charge_text,
                toggle_debug,
                apply_hud,
                report_outcome,
                camera_controls,
            )
                .chain(),
        )
        .run();
}

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn in_paint_mode(mode: Res<Mode>) -> bool {
    *mode == Mode::Paint
}

fn in_run_mode(mode: Res<Mode>) -> bool {
    *mode == Mode::Run
}

/// Scroll to zoom, WASD to pan. Keeps the full grid available while letting you
/// zoom in far enough to read the itinerary arrows.
fn camera_controls(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut wheel: MessageReader<MouseWheel>,
    mut camera: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    let mut zoom = 0.0;
    for event in wheel.read() {
        zoom += event.y;
    }
    let pan_speed = 600.0 * time.delta_secs();

    for (mut transform, mut projection) in &mut camera {
        let mut scale = 1.0;
        if let Projection::Orthographic(ortho) = &mut *projection {
            if zoom != 0.0 {
                ortho.scale = (ortho.scale * 0.9_f32.powf(zoom)).clamp(0.15, 4.0);
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
    }
}
