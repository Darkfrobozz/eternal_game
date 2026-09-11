//! Sound effects for how a run ends.
//!
//! * the ball dies ([`crate::ball::Outcome::Depleted`] / [`crate::ball::Outcome::Stuck`])
//!   — the small `explosion_small.wav`,
//! * the whole level overloads ([`crate::ball::Outcome::Victory`]) — the big
//!   `explosion_boom.wav`.
//!
//! The cues are triggered by [`crate::explosion::spawn_explosion`], which already
//! fires exactly once per run.
//!
//! **Linux builds are silent.** WSLg's audio stack (PulseAudio and its ALSA
//! bridge) lags and drops out, so playback is compiled out there with `cfg`,
//! and [`crate::main`] drops Bevy's `AudioPlugin` entirely. The `Sfx` resource
//! is still created so the rest of the game does not care which platform it is
//! running on.

use bevy::prelude::*;

#[cfg(not(target_os = "linux"))]
use bevy::audio::Volume;

/// Background rain volume. Kept below the one-shot explosion cues so it sits
/// under them rather than competing.
#[cfg(not(target_os = "linux"))]
const RAIN_VOLUME: f32 = 0.4;

/// Handles for the one-shot sound effects, loaded once at startup. The looping
/// rain handle is local to [`setup_sfx`] because nothing else needs it.
#[derive(Resource, Default)]
pub struct Sfx {
    /// The ball's death burst: a small explosion.
    pub death: Handle<AudioSource>,
    /// The victory overload: the big explosion.
    pub victory: Handle<AudioSource>,
}

/// Load the sound effects and start the looping rain.
#[cfg(not(target_os = "linux"))]
pub fn setup_sfx(mut commands: Commands, assets: Res<AssetServer>) {
    // Rain runs for the life of the app, across the menu and every level.
    let rain = assets.load("sfx/rain_loop.wav");
    commands.spawn((
        AudioPlayer::new(rain),
        PlaybackSettings::LOOP.with_volume(Volume::Linear(RAIN_VOLUME)),
    ));
    commands.insert_resource(Sfx {
        death: assets.load("sfx/explosion_small.wav"),
        victory: assets.load("sfx/explosion_boom.wav"),
    });
}

/// Linux build: no audio device is opened (see the module docs), so just keep
/// the resource present with empty handles.
#[cfg(target_os = "linux")]
pub fn setup_sfx(mut commands: Commands) {
    commands.insert_resource(Sfx::default());
}

/// Play a one-shot cue, despawning its entity when the sound finishes.
#[cfg(not(target_os = "linux"))]
pub fn play(commands: &mut Commands, handle: &Handle<AudioSource>) {
    commands.spawn((
        AudioPlayer::new(handle.clone()),
        PlaybackSettings::DESPAWN,
    ));
}

/// Linux build: silent no-op (see the module docs).
#[cfg(target_os = "linux")]
pub fn play(_commands: &mut Commands, _handle: &Handle<AudioSource>) {}
