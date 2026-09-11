//! Screen-anchored [`Text2d`] helpers.
//!
//! `Text2d` lives in world space, so a camera zoom or pan would normally
//! carry it around the board. [`ScreenText`] marks a label that should instead
//! stay pinned to the window; [`position_screen_text`] re-reads the camera
//! every frame and also scales the text so its pixel size stays constant.

use bevy::prelude::*;

/// Where on the window a [`ScreenText`] is pinned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenAnchor {
    /// Top-left corner, inset by `margin` pixels.
    TopLeft,
    /// Dead centre of the window.
    Center,
}

/// A `Text2d` that stays fixed to the window.
///
/// Spawners should pair this with an [`Anchor`](bevy::sprite::Anchor) that
/// matches the chosen [`ScreenAnchor`] (e.g. `Anchor::TOP_LEFT` for
/// [`ScreenAnchor::TopLeft`]), because the system positions the entity origin.
#[derive(Component)]
pub struct ScreenText {
    pub anchor: ScreenAnchor,
    /// Distance from the relevant window edge, in pixels.
    pub margin: f32,
    /// World z for draw order.
    pub z: f32,
}

/// Re-pin every [`ScreenText`] to the window, compensating for camera zoom and
/// pan so its position *and* size stay constant in pixels.
pub fn position_screen_text(
    camera: Single<(&Transform, &Projection), With<Camera2d>>,
    window: Single<&Window>,
    mut texts: Query<(&ScreenText, &mut Transform), Without<Camera2d>>,
) {
    let (camera_transform, projection) = *camera;
    let scale = match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    let half = Vec2::new(window.width(), window.height()) * 0.5 * scale;
    let center = camera_transform.translation.truncate();

    for (screen, mut transform) in &mut texts {
        let pos = match screen.anchor {
            ScreenAnchor::TopLeft => {
                center + Vec2::new(-half.x + screen.margin * scale, half.y - screen.margin * scale)
            }
            ScreenAnchor::Center => center,
        };
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
        transform.translation.z = screen.z;
        // Text2d is sized in world units; undo the camera's zoom to keep a
        // constant on-screen font size.
        transform.scale = Vec3::splat(scale);
    }
}
