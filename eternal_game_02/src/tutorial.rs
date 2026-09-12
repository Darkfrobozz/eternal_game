//! A guided tutorial for the core loop.
//!
//! The tutorial is active whenever a level whose file name contains
//! `tutorial` is loaded (see [`crate::config`]). It walks the player through
//! everything the puzzle assumes they know:
//!
//! 1. drawing solids on the grid,
//! 2. `Tab` to nudge the ball forward one cell,
//! 3. `Space` to start the ball rolling,
//! 4. `Space` again to pause it,
//! 5. the scroll wheel to zoom,
//! 6. `W`/`A`/`S`/`D` to pan,
//! 7. `-`/`=` to change the ball speed,
//! 8. closing an eternal loop,
//! 9. `E` to leave run mode.
//!
//! The objectives are a checklist, not a strict sequence: each is latched the
//! moment it happens, so the player is free to do them in any order.
//!
//! The prompt is drawn as [`Text2d`] but pinned to the top-left of the screen
//! every frame, so zooming and panning (the very things it teaches) can't
//! carry the instructions off-view.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::ball::{Run, Tuning};
use crate::grid::Grid;
use crate::paint::Mode;
use crate::screen::{ScreenAnchor, ScreenText};

/// The tutorial's objectives, in display order.
const OBJECTIVES: [&str; 9] = [
    "Draw on the grid (left-click and drag)",
    "Press TAB to nudge the ball one step",
    "Press SPACE to start rolling the ball",
    "Press SPACE again to pause the ball",
    "Scroll the mouse wheel to zoom in and out",
    "Hold W, A, S, D to pan the view",
    "Press - / = to change the ball speed",
    "Make the ball loop forever",
    "Press E to leave run mode",
];

/// Everything the tutorial can observe about a frame.
#[derive(Default, Clone, Copy)]
struct Observed {
    drawn: bool,
    stepped: bool,
    rolled: bool,
    paused: bool,
    exited: bool,
    zoomed: bool,
    panned: bool,
    speed_changed: bool,
    looped: bool,
}

/// Progress through the tutorial. Each flag latches once its action is seen.
#[derive(Resource, Default)]
pub struct Tutorial {
    /// True while a tutorial level is loaded.
    active: bool,
    drawn: bool,
    stepped: bool,
    rolled: bool,
    paused: bool,
    exited: bool,
    zoomed: bool,
    panned: bool,
    speed_changed: bool,
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
        self.flags().iter().all(|done| *done)
    }

    /// Per-objective latches, in [`OBJECTIVES`] order.
    fn flags(&self) -> [bool; OBJECTIVES.len()] {
        [
            self.drawn,
            self.stepped,
            self.rolled,
            self.paused,
            self.zoomed,
            self.panned,
            self.speed_changed,
            self.looped,
            self.exited,
        ]
    }

    /// Latch the actions seen this frame. Split out from the system so it can
    /// be unit-tested.
    fn observe(&mut self, observed: Observed) {
        self.drawn |= observed.drawn;
        self.stepped |= observed.stepped;
        self.rolled |= observed.rolled;
        self.paused |= observed.paused;
        self.exited |= observed.exited;
        self.zoomed |= observed.zoomed;
        self.panned |= observed.panned;
        self.speed_changed |= observed.speed_changed;
        self.looped |= observed.looped;
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

/// Watch the player and latch the current objective.
pub fn track_tutorial(
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<Mode>,
    grid: Res<Grid>,
    run: Res<Run>,
    tuning: Res<Tuning>,
    mut wheel: MessageReader<MouseWheel>,
    mut tutorial: ResMut<Tutorial>,
    mut last_steps: Local<usize>,
    mut last_mode: Local<Mode>,
) {
    // Always drain the wheel so a future tutorial doesn't read stale events.
    let scrolled = wheel.read().any(|event| event.y.abs() > f32::EPSILON);

    // `handle_mode` has already run this frame, so an `E` that left run mode
    // shows up here as a run -> pen transition.
    let was_running = *last_mode == Mode::Run;
    *last_mode = *mode;

    if !tutorial.active || tutorial.complete() {
        return;
    }

    let running = *mode == Mode::Run;
    let space = keys.just_pressed(KeyCode::Space);
    // Did the ball actually advance since last frame? `Tab` also *enters* run
    // mode, which does not count as a nudge, so compare the itinerary. When
    // the itinerary shrinks, a fresh run started this frame.
    let steps = run.itinerary.len();
    let took_step = if steps < *last_steps {
        steps > 0
    } else {
        steps > *last_steps
    };
    *last_steps = steps;

    // The pen can only add solids that the level did not lock, so any such
    // cell means the player has drawn something of their own.
    let drawn = grid.has_unlocked_solid();
    let panned = keys.any_just_pressed([
        KeyCode::KeyW,
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
    ]);
    tutorial.observe(Observed {
        drawn,
        stepped: keys.just_pressed(KeyCode::Tab) && took_step,
        // `Space` starts rolling when it leaves the ball in auto mode, and
        // pauses when it leaves it paused in manual mode.
        rolled: space && running && !tuning.manual,
        paused: space && running && tuning.manual,
        exited: keys.just_pressed(KeyCode::KeyE) && was_running,
        zoomed: scrolled,
        panned,
        speed_changed: keys.any_just_pressed([KeyCode::Minus, KeyCode::Equal]),
        looped: run.solved,
    });
}

/// Write the checklist into the prompt.
pub fn draw_tutorial(tutorial: Res<Tutorial>, mut texts: Query<&mut Text2d, With<TutorialText>>) {
    for mut text in &mut texts {
        if !tutorial.active {
            text.0.clear();
        } else if tutorial.complete() {
            text.0 =
                "TUTORIAL COMPLETE\n\nPress E to leave run mode,\nor PageDown for the next level.".into();
        } else {
            let flags = tutorial.flags();
            let mut body = String::from("TUTORIAL\n\n");
            for (objective, done) in OBJECTIVES.iter().zip(flags) {
                let mark = if done { 'x' } else { ' ' };
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

    fn all() -> Observed {
        Observed {
            drawn: true,
            stepped: true,
            rolled: true,
            paused: true,
            exited: true,
            zoomed: true,
            panned: true,
            speed_changed: true,
            looped: true,
        }
    }

    #[test]
    fn objectives_latch_independently() {
        let mut tutorial = Tutorial {
            active: true,
            ..default()
        };

        // Only scroll: exactly that box is ticked, nothing else.
        tutorial.observe(Observed {
            zoomed: true,
            ..default()
        });
        assert_eq!(
            tutorial.flags(),
            [false, false, false, false, true, false, false, false, false]
        );
        assert!(!tutorial.complete());

        // Looping before the run controls still latches.
        tutorial.observe(Observed {
            looped: true,
            ..default()
        });
        assert_eq!(
            tutorial.flags(),
            [false, false, false, false, true, false, false, true, false]
        );
    }

    #[test]
    fn each_action_latches_its_own_objective() {
        // (observed action, index in OBJECTIVES it should tick)
        let cases: [(Observed, usize); 9] = [
            (
                Observed {
                    drawn: true,
                    ..default()
                },
                0,
            ),
            (
                Observed {
                    stepped: true,
                    ..default()
                },
                1,
            ),
            (
                Observed {
                    rolled: true,
                    ..default()
                },
                2,
            ),
            (
                Observed {
                    paused: true,
                    ..default()
                },
                3,
            ),
            (
                Observed {
                    zoomed: true,
                    ..default()
                },
                4,
            ),
            (
                Observed {
                    panned: true,
                    ..default()
                },
                5,
            ),
            (
                Observed {
                    speed_changed: true,
                    ..default()
                },
                6,
            ),
            (
                Observed {
                    looped: true,
                    ..default()
                },
                7,
            ),
            (
                Observed {
                    exited: true,
                    ..default()
                },
                8,
            ),
        ];

        for (observed, expected) in cases {
            let mut tutorial = Tutorial {
                active: true,
                ..default()
            };
            tutorial.observe(observed);
            for (i, done) in tutorial.flags().iter().enumerate() {
                assert_eq!(
                    *done,
                    i == expected,
                    "objective {i} ({}) latched by the wrong action",
                    OBJECTIVES[i]
                );
            }
        }
    }

    #[test]
    fn all_objectives_together_complete_it() {
        let mut tutorial = Tutorial {
            active: true,
            ..default()
        };
        tutorial.observe(all());
        assert!(tutorial.complete());
    }

    #[test]
    fn loading_a_non_tutorial_level_clears_progress() {
        let mut tutorial = Tutorial::default();
        tutorial.set_level(true);
        tutorial.observe(all());
        assert!(tutorial.complete());

        tutorial.set_level(false);
        assert!(!tutorial.active);
        assert!(!tutorial.complete());
    }

    #[test]
    fn reloading_a_tutorial_restarts_it() {
        let mut tutorial = Tutorial::default();
        tutorial.set_level(true);
        tutorial.observe(all());
        tutorial.set_level(true);
        assert!(!tutorial.complete());
    }
}
