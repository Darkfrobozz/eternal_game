//! The title menu and the top-level [`Screen`] state.
//!
//! The game starts on the menu:
//!
//! - **Continue Game** resumes the last game level the player loaded,
//! - **New Game** starts at the first non-tutorial level,
//! - **Tutorial** loads the guided level,
//! - **Map Editor** drops you on a blank board with the editor HUD on.
//!
//! `Esc` clears the board and returns to the menu from a game or the editor.
//! Progress is written to `progress.txt` by [`crate::config::load_level`]
//! whenever a game level is loaded, so Continue survives a restart.

use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::ball::{Ball, Run, Tuning};
use crate::config::{self, Levels, Progress};
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

/// A selectable menu entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Choice {
    Continue,
    NewGame,
    Tutorial,
    Editor,
}

impl Choice {
    fn label(self) -> &'static str {
        match self {
            Choice::Continue => "Continue Game",
            Choice::NewGame => "New Game",
            Choice::Tutorial => "Tutorial",
            Choice::Editor => "Map Editor",
        }
    }
}

/// The visible menu entries, in order. Continue only appears once there is
/// saved progress to resume.
fn choices(has_save: bool) -> Vec<Choice> {
    let mut items = Vec::new();
    if has_save {
        items.push(Choice::Continue);
    }
    items.extend([Choice::NewGame, Choice::Tutorial, Choice::Editor]);
    items
}

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
    progress: Res<Progress>,
    mut texts: Query<(&mut Text2d, &mut Visibility), With<MenuText>>,
) {
    let items = choices(progress.saved.is_some());
    for (mut text, mut visibility) in &mut texts {
        if *screen != Screen::Menu {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Visible;

        let selected = menu.selected.min(items.len().saturating_sub(1));
        let mut body = String::from("ETERNAL GAME 02\n\n");
        for (i, item) in items.iter().enumerate() {
            let cursor = if i == selected { '>' } else { ' ' };
            body.push_str(&format!("{cursor} {}\n", item.label()));
        }
        body.push_str("\nUp/Down or W/S to choose\n1-4 to pick directly   Enter to start");
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
    mut progress: ResMut<Progress>,
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

    let items = choices(progress.saved.is_some());
    let len = items.len();
    menu.selected = menu.selected.min(len - 1);

    if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) {
        menu.selected = (menu.selected + 1) % len;
    }
    if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) {
        menu.selected = (menu.selected + len - 1) % len;
    }

    let mut direct = None;
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
    ];
    for (i, key) in digits.into_iter().enumerate() {
        if keys.just_pressed(key) {
            direct = Some(i);
        }
    }
    if let Some(i) = direct.filter(|i| *i < len) {
        menu.selected = i;
    }

    let confirm = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space);
    if !confirm && direct.is_none() {
        return;
    }

    let selected = items[menu.selected];

    // Pick the level before touching the board, so the saved name is read
    // before `load_level` overwrites it.
    *levels = Levels::scan();
    let path = match selected {
        Choice::Continue => progress
            .saved
            .clone()
            .and_then(|name| levels.find(&name))
            .or_else(|| levels.first_game()),
        Choice::NewGame => levels.first_game(),
        Choice::Tutorial => levels.tutorial(),
        Choice::Editor => None,
    };

    if selected == Choice::Editor {
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
        return;
    }

    // Every fresh game starts from a default view.
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
    match path {
        Some(path) => config::load_level(
            &path,
            &mut grid,
            &mut place,
            &mut tuning,
            &mut run,
            &mut mode,
            &mut tutorial,
            &mut progress,
        ),
        None => warn!("No level files found in levels/"),
    }
    *screen = Screen::Game;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_without_progress_has_no_continue() {
        assert_eq!(
            choices(false),
            vec![Choice::NewGame, Choice::Tutorial, Choice::Editor]
        );
        assert_eq!(Menu::default().selected, 0);
    }

    #[test]
    fn continue_is_first_once_there_is_a_save() {
        assert_eq!(
            choices(true),
            vec![
                Choice::Continue,
                Choice::NewGame,
                Choice::Tutorial,
                Choice::Editor
            ]
        );
    }
}
