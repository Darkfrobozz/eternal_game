//! A guided tutorial for the core loop.
//!
//! The tutorial is active whenever a level whose file name contains
//! `tutorial` is loaded (see [`crate::config`]). It walks the player through
//! everything the puzzle assumes they know:
//!
//! 1. drawing solids on the grid,
//! 2. `Space` to start the ball rolling,
//! 3. the scroll wheel to zoom,
//! 4. `W`/`A`/`S`/`D` to pan,
//! 5. closing an eternal loop.
//!
//! The prompt is drawn as [`Text2d`] but pinned to the top-left of the screen
//! every frame, so zooming and panning (the very things it teaches) can't
//! carry the instructions off-view.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::ball::{Outcome, Run};
use crate::grid::Grid;
use crate::paint::Mode;
use crate::screen::{ScreenAnchor, ScreenText};

/// The tutorial's objectives, in the order they are introduced.
const OBJECTIVES: [&str; 5] = [
    "Draw on the grid (left-click and drag)",
    "Press SPACE to start rolling the ball",
    "Scroll the mouse wheel to zoom in and out",
    "Hold W, A, S, D to pan the view",
    "Make the ball loop forever",
];

/// Everything the tutorial can observe about a frame.
#[derive(Default, Clone, Copy)]
struct Observed {
    drawn: bool,
    rolled: bool,
    zoomed: bool,
    panned: bool,
    looped: bool,
}

/// Progress through the tutorial.
///
/// `step` is the objective currently being taught. Because the player may do
/// them out of order (for example scroll before drawing), each action is
/// latched and completed objectives are skipped as soon as the player reaches
/// them.
#[derive(Resource, Default)]
pub struct Tutorial {
    /// True while a tutorial level is loaded.
    active: bool,
    /// Index of the current objective; `== OBJECTIVES.len()` when complete.
    step: usize,
    drawn: bool,
    rolled: bool,
    zoomed: bool,
    panned: bool,
    looped: bool,
}

impl Tutorial {
    /// Called whenever a level is loaded. Loading a tutorial level (re)starts
    /// the walkthrough; any other level switches it off.
    pub fn set_level(&mut self, tutorial_level: bool) {
        *self = if tutorial_level {
            Self {
                active: true,
                ..default()
            }
        } else {
            Self::default()
        };
    }

    /// Has every objective been met?
    pub fn complete(&self) -> bool {
        self.step >= OBJECTIVES.len()
    }

    /// Latch the actions seen this frame and advance past every objective that
    /// is now satisfied. Split out from the system so it can be unit-tested.
    fn observe(&mut self, observed: Observed) {
        self.drawn |= observed.drawn;
        self.rolled |= observed.rolled;
        self.zoomed |= observed.zoomed;
        self.panned |= observed.panned;
        self.looped |= observed.looped;
        while !self.complete() && self.current_done() {
            self.step += 1;
        }
    }

    fn current_done(&self) -> bool {
        match self.step {
            0 => self.drawn,
            1 => self.rolled,
            2 => self.zoomed,
            3 => self.panned,
            4 => self.looped,
            _ => true,
        }
    }
}

/// Marks the on-screen tutorial prompt.
#[derive(Component)]
pub struct TutorialText;

/// Spawn the (initially hidden) tutorial prompt.
pub fn setup_tutorial(mut commands: Commands) {
    commands.spawn((
        Text2d::new(""),
        TextFont {
            font_size: FontSize::Px(17.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.93, 0.55)),
        Anchor::TOP_LEFT,
        ScreenText {
            anchor: ScreenAnchor::TopLeft,
            margin: 16.0,
            z: 30.0,
        },
        Transform::from_xyz(0.0, 0.0, 30.0),
        TutorialText,
        Visibility::Hidden,
    ));
}

/// Watch the player and advance the current objective.
pub fn track_tutorial(
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<Mode>,
    grid: Res<Grid>,
    run: Res<Run>,
    mut wheel: MessageReader<MouseWheel>,
    mut tutorial: ResMut<Tutorial>,
) {
    // Always drain the wheel so a future tutorial doesn't read stale events.
    let scrolled = wheel.read().any(|event| event.y.abs() > f32::EPSILON);
    if !tutorial.active || tutorial.complete() {
        return;
    }

    // The pen can only add solids that the level did not lock, so any such
    // cell means the player has drawn something of their own.
    let drawn = grid
        .solids
        .iter()
        .any(|cell| !grid.locked.contains(cell));
    let panned = keys.any_just_pressed([
        KeyCode::KeyW,
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
    ]);
    tutorial.observe(Observed {
        drawn,
        rolled: *mode == Mode::Run,
        zoomed: scrolled,
        panned,
        looped: run.outcome == Outcome::Won,
    });
}

/// Write the current objectives (and their check marks) into the prompt.
pub fn draw_tutorial(tutorial: Res<Tutorial>, mut texts: Query<&mut Text2d, With<TutorialText>>) {
    for mut text in &mut texts {
        if !tutorial.active {
            text.0.clear();
        } else if tutorial.complete() {
            text.0 =
                "TUTORIAL COMPLETE\n\nPress SPACE to stop the ball,\nor TAB for the next level.".into();
        } else {
            let mut body = String::from("TUTORIAL\n\n");
            for (i, objective) in OBJECTIVES.iter().enumerate() {
                let mark = if i < tutorial.step { 'x' } else { ' ' };
                body.push_str(&format!("[{mark}] {objective}\n"));
            }
            text.0 = body;
        }
    }
}

/// Show the prompt only on tutorial levels.
pub fn apply_tutorial_visibility(
    tutorial: Res<Tutorial>,
    mut texts: Query<&mut Visibility, With<TutorialText>>,
) {
    let target = if tutorial.active {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut texts {
        *visibility = target;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objectives_can_be_completed_out_of_order() {
        let mut tutorial = Tutorial {
            active: true,
            ..default()
        };

        // Scroll first: it is latched, but drawing is still the current goal.
        tutorial.observe(Observed {
            zoomed: true,
            ..default()
        });
        assert_eq!(tutorial.step, 0);

        // Drawing advances past goal 0 but stops at starting the ball.
        tutorial.observe(Observed {
            drawn: true,
            ..default()
        });
        assert_eq!(tutorial.step, 1);

        // Space + pan + a completed loop then sweep the remaining goals,
        // including the scroll that was already latched.
        tutorial.observe(Observed {
            rolled: true,
            panned: true,
            looped: true,
            ..default()
        });
        assert_eq!(tutorial.step, 5);
        assert!(tutorial.complete());
    }

    #[test]
    fn loading_a_non_tutorial_level_clears_progress() {
        let mut tutorial = Tutorial::default();
        tutorial.set_level(true);
        tutorial.observe(Observed {
            drawn: true,
            rolled: true,
            zoomed: true,
            panned: true,
            looped: true,
        });
        assert!(tutorial.complete());

        tutorial.set_level(false);
        assert!(!tutorial.active);
        assert_eq!(tutorial.step, 0);
        assert!(!tutorial.complete());
    }

    #[test]
    fn reloading_a_tutorial_restarts_it() {
        let mut tutorial = Tutorial::default();
        tutorial.set_level(true);
        tutorial.observe(Observed {
            drawn: true,
            rolled: true,
            zoomed: true,
            panned: true,
            looped: true,
        });
        tutorial.set_level(true);
        assert_eq!(tutorial.step, 0);
        assert!(!tutorial.complete());
    }
}
