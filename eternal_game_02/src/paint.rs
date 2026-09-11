//! Painting the grid, and switching between pen and run modes.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::ball::{self, Ball, Outcome, Run, Tuning};
use crate::grid::{CELL_PX, Cell, GRID_H, GRID_W, Grid};

/// Which half of the game is active.
#[derive(Resource, PartialEq, Eq, Clone, Copy, Default, Debug)]
pub enum Mode {
    /// Draw solids; the ball is hidden.
    #[default]
    Paint,
    /// Watch the ball roll.
    Run,
}

/// Remembers the previous painted cell so a fast drag paints a continuous line.
#[derive(Resource, Default)]
pub struct Brush {
    last: Option<IVec2>,
}

/// Where the player has chosen to drop the ball (for testing). `None` means
/// "use the auto start" (the topmost surface cell).
#[derive(Resource, Default)]
pub struct Placement {
    pub start: Option<IVec2>,
}

/// The on-board indicator for a manually chosen start.
#[derive(Component)]
pub struct StartMarker;

/// Create the backing image, the sprite that displays it, and the `Grid`.
pub fn setup_grid(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new_fill(
        Extent3d {
            width: GRID_W as u32,
            height: GRID_H as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &Grid::color(Cell::Empty),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    let handle = images.add(image);

    let grid = Grid::new(handle.clone());
    commands.spawn(Sprite {
        image: handle,
        custom_size: Some(grid.world_size()),
        ..default()
    });
    commands.insert_resource(grid);

    commands.spawn((
        Text2d::new(
            "Space: pen/run   Left-drag: draw   Right-drag: erase   Middle-click: place   B: auto start   [ ]: start charge\nN: step   M: auto/manual   scroll: zoom   WASD: pan   Y/L: save/load   C: clear",
        ),
        TextFont {
            font_size: FontSize::Px(15.0),
            ..default()
        },
        TextColor(Color::srgb(0.65, 0.70, 0.82)),
        Transform::from_xyz(0.0, GRID_H as f32 * CELL_PX / 2.0 - 16.0, 10.0),
    ));

    // Marker for the manually placed start (hidden until used).
    commands.spawn((
        StartMarker,
        Sprite::from_color(Color::srgb(0.35, 0.75, 1.0), Vec2::splat(CELL_PX * 0.9)),
        Transform::from_xyz(0.0, 0.0, 4.0),
        Visibility::Hidden,
    ));

    ball::spawn_charge_text(&mut commands);
}

/// Cursor position -> grid cell, if it is over the board.
fn cursor_cell(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    window: &Window,
    grid: &Grid,
) -> Option<IVec2> {
    let cursor = window.cursor_position()?;
    let world = camera
        .viewport_to_world_2d(camera_transform, cursor)
        .ok()?;
    grid.world_to_cell(world)
}

/// Middle-click drops the ball on a surface cell during pen mode; `B` reverts
/// to the automatic start.
#[allow(clippy::too_many_arguments)]
pub fn place_start(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    window: Single<&Window>,
    mode: Res<Mode>,
    grid: Res<Grid>,
    mut placement: ResMut<Placement>,
    mut marker: Query<(&mut Transform, &mut Visibility), With<StartMarker>>,
) {
    if keys.just_pressed(KeyCode::KeyB) {
        placement.start = None;
    }
    if *mode == Mode::Paint && mouse.just_pressed(MouseButton::Middle) {
        let (camera, camera_transform) = *camera;
        if let Some(cell) = cursor_cell(camera, camera_transform, &window, &grid)
            && grid.is_track(cell)
        {
            placement.start = Some(cell);
        }
    }

    let visible = if *mode == Mode::Paint {
        placement.start
    } else {
        None
    }
    .filter(|cell| grid.is_track(*cell));

    if let Ok((mut transform, mut visibility)) = marker.single_mut() {
        match visible {
            Some(cell) => {
                let pos = grid.cell_to_world(cell);
                transform.translation.x = pos.x;
                transform.translation.y = pos.y;
                *visibility = Visibility::Visible;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

/// Space toggles pen/run. Entering run resets the trail and drops the ball on
/// the start; entering pen despawns the ball and clears the trail.
pub fn handle_mode(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut mode: ResMut<Mode>,
    mut grid: ResMut<Grid>,
    mut run: ResMut<Run>,
    placement: Res<Placement>,
    tuning: Res<Tuning>,
    balls: Query<Entity, With<Ball>>,
) {
    if !keys.just_pressed(KeyCode::Space) {
        return;
    }

    match *mode {
        Mode::Paint => {
            *mode = Mode::Run;
            grid.reset_trail();
            *run = Run::default();
            let chosen = placement
                .start
                .filter(|cell| grid.is_track(*cell))
                .or_else(|| grid.find_start());
            match chosen {
                Some(start) => {
                    ball::start_run(&mut run, &grid, start, tuning.start_charge);
                    ball::spawn_ball(&mut commands, &grid, start, tuning.start_charge);
                }
                None => info!("No surface to run on yet — draw something first."),
            }
        }
        Mode::Run => {
            *mode = Mode::Paint;
            for entity in &balls {
                commands.entity(entity).despawn();
            }
            grid.reset_trail();
        }
    }
}

/// Blit the array into the texture, but only when something changed.
pub fn sync_image(mut grid: ResMut<Grid>, mut images: ResMut<Assets<Image>>) {
    if !grid.dirty {
        return;
    }
    let Some(mut image) = images.get_mut(&grid.image) else {
        return;
    };
    let Some(data) = image.data.as_mut() else {
        return;
    };
    // Guard against a stale/placeholder texture (e.g. a bad load).
    if data.len() < (grid.w * grid.h * 4) as usize {
        return;
    }

    for y in 0..grid.h {
        for x in 0..grid.w {
            let cell = grid.cells[(y * grid.w + x) as usize];
            // Image row 0 is the top, grid row 0 is the bottom.
            let row = grid.h - 1 - y;
            let offset = ((row * grid.w + x) as usize) * 4;
            data[offset..offset + 4].copy_from_slice(&Grid::color(cell));
        }
    }
    grid.dirty = false;
}

/// Paint (or erase) solids under the cursor while a mouse button is held.
pub fn paint(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    window: Single<&Window>,
    mut grid: ResMut<Grid>,
    mut brush: ResMut<Brush>,
) {
    if keys.just_pressed(KeyCode::KeyC) {
        grid.clear();
    }

    let painting = mouse.pressed(MouseButton::Left);
    let erasing = mouse.pressed(MouseButton::Right);
    if !painting && !erasing {
        brush.last = None;
        return;
    }

    let (camera, camera_transform) = *camera;
    let Some(cursor) = window.cursor_position() else {
        brush.last = None;
        return;
    };
    let Ok(world) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        brush.last = None;
        return;
    };
    let Some(cell) = grid.world_to_cell(world) else {
        brush.last = None;
        return;
    };

    let value = if erasing { Cell::Empty } else { Cell::Solid };
    match brush.last {
        Some(prev) => grid.paint_line(prev, cell, value),
        None => grid.paint(cell, value),
    }
    brush.last = Some(cell);
}

/// Once a run has finished, log it just once (placeholder for level UI).
pub fn report_outcome(run: Res<Run>, mut reported: Local<bool>) {
    if run.outcome == Outcome::Running {
        *reported = false;
    } else if !*reported {
        *reported = true;
        info!("Run finished: {:?}", run.outcome);
    }
}
