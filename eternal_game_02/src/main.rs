//! Eternal Game 02 — a puzzle game about drawing surfaces and building charge.
//!
//! Phase 1: the [`Grid`] array plus a mouse brush.
//! Phase 2: the pen grows surface around solids, and a ball walks the
//! flood-filled route clockwise while accumulating charge.

mod ball;
mod grid;
mod paint;

use bevy::prelude::*;

use ball::{Run, Tuning, step_ball, tune_start_charge, update_ball_transform, update_charge_text};
use grid::{CELL_PX, GRID_H, GRID_W};
use paint::{
    Brush, Mode, Placement, handle_mode, paint, place_start, report_outcome, setup_grid, sync_image,
};

fn main() {
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
        .add_systems(Startup, (setup_grid, setup_camera))
        .add_systems(
            Update,
            (
                tune_start_charge,
                place_start,
                handle_mode,
                paint.run_if(in_paint_mode),
                step_ball.run_if(in_run_mode),
                sync_image,
                update_ball_transform,
                update_charge_text,
                report_outcome,
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
