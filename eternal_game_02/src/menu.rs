//! The title menu and the top-level [`Screen`] state.
//!
//! The game starts on the menu. **New Game** loads the first level (the
//! tutorial); **Map Editor** drops you on a blank board with the editor HUD on
//! so you can draw a level and press `Y` to save it to `debug_config.txt`.
//! `Esc` returns to the menu from either, clearing the board.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::ball::{Ball, Run, Tuning};
use crate::config::{self, Levels};
use crate::grid::Grid;
use crate::paint::{Debug, Mode, Placement};
use crate::screen::{ScreenAnchor, ScreenText};
use crate::tutorial::Tutorial;

/// Which top-level screen is showing.
#[derive(Resource, PartialEq, Eq, Clone, Copy, Debug, Default)]
pub enum Screen {
    #[default]
    Menu,
    Game,
    Editor,
}

/// The two menu entries, top to bottom.
const ITEMS: [&str; 2] = ["New Game", "Map Editor"];

/// Menu cursor state.
#[derive(Resource, Default)]
pub struct Menu {
    pub selected: usize,
}

/// Marks the menu label.
#[derive(Component)]
pub struct MenuText;

/// Spawn the (hidden until the menu is active) title text, centred on screen.
pub fn setup_menu(mut commands: Commands) {
    commands.spawn((
        Text2d::new(""),
        TextFont {
            font_size: FontSize::Px(24.0),
            ..default()
        },
        TextColor(Color::srgb(0.90, 0.94, 1.0)),
        TextLayout::justify(Justify::Center),
        Anchor::CENTER,
        ScreenText {
            anchor: ScreenAnchor::Center,
            margin: 0.0,
            z: 40.0,
        },
        Transform::from_xyz(0.0, 0.0, 40.0),
        MenuText,
        Visibility::Hidden,
    ));
}

/// Render the menu, or hide it when a game/editor is active.
pub fn draw_menu(
    screen: Res<Screen>,
    menu: Res<Menu>,
    mut texts: Query<(&mut Text2d, &mut Visibility), With<MenuText>>,
) {
    for (mut text, mut visibility) in &mut texts {
        if *screen != Screen::Menu {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Visible;

        let mut body = String::from("ETERNAL GAME 02\n\n");
        for (i, item) in ITEMS.iter().enumerate() {
            let cursor = if i == menu.selected { '>' } else { ' ' };
            body.push_str(&format!("{cursor} {item}\n"));
        }
        body.push_str("\nUp/Down or W/S to choose\n1 / 2 to pick directly   Enter to start");
        text.0 = body;
    }
}

/// Reset the board to an empty slate and put the camera back to its default.
/// `editor` decides whether the debug/editor HUD comes back on.
#[allow(clippy::too_many_arguments)]
fn blank_slate(
    grid: &mut Grid,
    place: &mut Placement,
    tuning: &mut Tuning,
    run: &mut Run,
    mode: &mut Mode,
    debug: &mut Debug,
    tutorial: &mut Tutorial,
    cameras: &mut Query<(&mut Transform, &mut Projection), With<Camera2d>>,
    editor: bool,
) {
    *grid = Grid::new(grid.image.clone());
    *place = Placement::default();
    *tuning = Tuning::default();
    *run = Run::default();
    *mode = Mode::Paint;
    *debug = Debug(editor);
    *tutorial = Tutorial::default();
    for (mut transform, mut projection) in cameras.iter_mut() {
        transform.translation = Vec3::ZERO;
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scale = 1.0;
        }
    }
}

/// Move the cursor and confirm a choice; `Esc` clears the board and returns to
/// the menu.
#[allow(clippy::too_many_arguments)]
pub fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut screen: ResMut<Screen>,
    mut menu: ResMut<Menu>,
    mut levels: ResMut<Levels>,
    mut grid: ResMut<Grid>,
    mut place: ResMut<Placement>,
    mut tuning: ResMut<Tuning>,
    mut run: ResMut<Run>,
    mut mode: ResMut<Mode>,
    mut debug: ResMut<Debug>,
    mut tutorial: ResMut<Tutorial>,
    balls: Query<Entity, With<Ball>>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    // From anywhere, Escape clears the board and drops back to the title menu.
    if *screen != Screen::Menu {
        if keys.just_pressed(KeyCode::Escape) {
            for entity in &balls {
                commands.entity(entity).despawn();
            }
            blank_slate(
                &mut grid,
                &mut place,
                &mut tuning,
                &mut run,
                &mut mode,
                &mut debug,
                &mut tutorial,
                &mut cameras,
                false,
            );
            *screen = Screen::Menu;
        }
        return;
    }

    if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) {
        menu.selected = (menu.selected + 1) % ITEMS.len();
    }
    if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) {
        menu.selected = (menu.selected + ITEMS.len() - 1) % ITEMS.len();
    }

    let direct = if keys.just_pressed(KeyCode::Digit1) {
        Some(0)
    } else if keys.just_pressed(KeyCode::Digit2) {
        Some(1)
    } else {
        None
    };
    if let Some(i) = direct {
        menu.selected = i;
    }

    let confirm = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space);
    if !confirm && direct.is_none() {
        return;
    }

    match menu.selected {
        0 => {
            // Re-scan so levels added while the app was open are picked up.
            *levels = Levels::scan();
            blank_slate(
                &mut grid,
                &mut place,
                &mut tuning,
                &mut run,
                &mut mode,
                &mut debug,
                &mut tutorial,
                &mut cameras,
                false,
            );
            match levels.first() {
                Some(path) => config::load_level(
                    &path,
                    &mut grid,
                    &mut place,
                    &mut tuning,
                    &mut run,
                    &mut mode,
                    &mut tutorial,
                ),
                None => warn!("No level files found in levels/"),
            }
            *screen = Screen::Game;
        }
        _ => {
            // Blank slate with the editor HUD on.
            blank_slate(
                &mut grid,
                &mut place,
                &mut tuning,
                &mut run,
                &mut mode,
                &mut debug,
                &mut tutorial,
                &mut cameras,
                true,
            );
            *screen = Screen::Editor;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_starts_on_new_game() {
        assert_eq!(Screen::default(), Screen::Menu);
        assert_eq!(Menu::default().selected, 0);
    }
}
