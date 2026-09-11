//! Sound effects for how a run ends.
//!
//! * the ball dies ([`crate::ball::Outcome::Depleted`] / [`crate::ball::Outcome::Stuck`])
//!   — the small `explosion_small.wav`,
//! * the whole level overloads ([`crate::ball::Outcome::Victory`]) — the same
//!   big `explosion_boom.wav` as the intro.
//!
//! The cues are triggered by [`crate::explosion::spawn_explosion`], which already
//! fires exactly once per run.

use bevy::audio::Volume;
use bevy::prelude::*;

/// Background rain volume. Kept below the one-shot explosion cues so it sits
/// under them rather than competing.
const RAIN_VOLUME: f32 = 0.4;

/// Handles for the sound effects, loaded once at startup.
#[derive(Resource)]
pub struct Sfx {
    /// Intro cue played as soon as the game opens.
    pub startup: Handle<AudioSource>,
    /// The ball's death burst: a small explosion.
    pub death: Handle<AudioSource>,
    /// The victory overload: the big explosion.
    pub victory: Handle<AudioSource>,
    /// Ambient rain, looped for the whole session.
    pub rain: Handle<AudioSource>,
}

/// Load the sound effects, play the intro cue, and start the looping rain. A
/// missing file just logs a load error; the game carries on (more) silently.
pub fn setup_sfx(mut commands: Commands, assets: Res<AssetServer>) {
    let sfx = Sfx {
        startup: assets.load("sfx/explosion_boom.wav"),
        death: assets.load("sfx/explosion_small.wav"),
        victory: assets.load("sfx/explosion_boom.wav"),
        rain: assets.load("sfx/rain_loop.wav"),
    };
    // Bevy holds the player until the asset finishes loading, so this fires as
    // soon as the first frame can decode it.
    play(&mut commands, &sfx.startup);
    // Rain runs for the life of the app, across the menu and every level.
    commands.spawn((
        AudioPlayer::new(sfx.rain.clone()),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(RAIN_VOLUME)),
    ));
    commands.insert_resource(sfx);
}

/// Play a one-shot cue, despawning its entity when the sound finishes.
pub fn play(commands: &mut Commands, handle: &Handle<AudioSource>) {
    commands.spawn((
        AudioPlayer::new(handle.clone()),
        PlaybackSettings::DESPAWN,
    ));
}
