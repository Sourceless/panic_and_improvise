//! Sounds that start a moment from now: a reload that begins once the gun has dropped away, or a
//! bullet's thump, which arrives at the speed of sound.
use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, SpatialScale, Volume};
use bevy::prelude::*;

/// The speed of sound, m/s.
pub const SPEED_OF_SOUND: f32 = 343.0;

/// How long a sound from `distance` metres away takes to arrive, seconds.
pub fn arrival_delay(distance: f32) -> f32 {
    distance.max(0.0) / SPEED_OF_SOUND
}

/// A sound waiting to start.
#[derive(Component)]
pub struct DelayedSound {
    pub delay: f32,
    pub sound: Handle<AudioSource>,
    pub speed: f32,
    pub volume: f32,
    /// Where it comes from, if it should be heard from a place (it gets quieter with distance and
    /// pans left or right); `None` plays it flat, as if inside the player's head.
    pub position: Option<Vec3>,
}

/// How much the distance to a placed sound is shrunk before the listener's ears work out how
/// quiet it is: an impact 100 m off should still be heard, if faintly.
const SPATIAL_SCALE: f32 = 0.12;

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, start_due_sounds);
    }
}

fn start_due_sounds(mut commands: Commands, time: Res<Time>, mut waiting: Query<(Entity, &mut DelayedSound)>) {
    for (entity, mut pending) in &mut waiting {
        pending.delay -= time.delta_secs();
        if pending.delay > 0.0 {
            continue;
        }
        let mut sound = commands.entity(entity);
        sound.remove::<DelayedSound>();
        let mut settings = PlaybackSettings { speed: pending.speed, volume: Volume::Linear(pending.volume), ..PlaybackSettings::DESPAWN };
        if let Some(position) = pending.position {
            settings.spatial = true;
            settings.spatial_scale = Some(SpatialScale::new(SPATIAL_SCALE));
            sound.insert(Transform::from_translation(position));
        }
        sound.insert((AudioPlayer(pending.sound.clone()), settings));
    }
}

/// Starts `sound` after `delay` seconds.
pub fn play_after(commands: &mut Commands, delay: f32, sound: Handle<AudioSource>, speed: f32, volume: f32, position: Option<Vec3>) {
    commands.spawn(DelayedSound { delay, sound, speed, volume, position });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_takes_about_three_milliseconds_a_metre() {
        assert!((arrival_delay(343.0) - 1.0).abs() < 1e-6);
        assert!((arrival_delay(100.0) - 0.2915).abs() < 0.001);
        assert_eq!(arrival_delay(0.0), 0.0);
        assert_eq!(arrival_delay(-5.0), 0.0);
    }
}
