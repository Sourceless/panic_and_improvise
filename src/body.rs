//! The body of the player, seen by the player: the legs and the trunk under the view, which the sun throws a shadow
//! of. (The arms are with the gun, in `weapon`.)

use bevy::prelude::*;

use crate::figure::{measure_gait, spawn_figure, FigureLook, Gait, Seen};
use crate::player::{player_movement, FpsCamera, Stance};

/// The body under the camera.
#[derive(Component)]
pub struct OwnBody;

/// How far, metres, back from the eye the body is. Under the chin, the top of the chest would be in front of the gun
/// in a view looking down, and hide it; this far back it is seen only when looking nearly straight down.
const SET_BACK: f32 = 0.3;
/// How high the model's eyes are: the head is made around this height, and is stretched so that it comes to the
/// height of the eye of the player.
const MODEL_EYE: f32 = 1.67;

pub struct BodyPlugin;

impl Plugin for BodyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_body, follow_body.after(player_movement).before(measure_gait)));
    }
}

fn spawn_body(mut commands: Commands, look: Option<Res<FigureLook>>, existing: Query<(), With<OwnBody>>, camera: Query<(), With<FpsCamera>>) {
    let (Some(look), true, true) = (look, existing.is_empty(), camera.single().is_ok()) else { return };
    commands.spawn((OwnBody, Gait::default(), Transform::default(), Visibility::default())).with_children(|body| spawn_figure(body, &look, Seen::FromWithin, 0, &[]));
}

/// Keeps the body under the eye, facing the way the player faces, as tall as the player is standing, crouching.
fn follow_body(camera: Query<(&Transform, &FpsCamera)>, mut bodies: Query<(&mut Transform, &mut Visibility), (With<OwnBody>, Without<FpsCamera>)>) {
    let (Ok((eye, cam)), Ok((mut body, mut visibility))) = (camera.single(), bodies.single_mut()) else { return };
    let (yaw, _, _) = eye.rotation.to_euler(EulerRot::YXZ);
    let behind = Vec2::new(yaw.sin(), yaw.cos()) * SET_BACK;
    let at = Vec2::new(eye.translation.x, eye.translation.z) - cam.lean_offset * 0.5 + behind;
    body.translation = Vec3::new(at.x, cam.view_feet, at.y);
    body.rotation = Quat::from_rotation_y(yaw);
    body.scale = Vec3::new(1.0, cam.eye_height / MODEL_EYE, 1.0);
    // Lying down, the body would be under the view and not behind it.
    *visibility = if cam.stance == Stance::Prone { Visibility::Hidden } else { Visibility::Inherited };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_follows_the_player_under_the_eye_and_ducks_with_them() {
        let mut app = App::new();
        app.init_resource::<Time>().add_systems(Update, follow_body);
        let cam = FpsCamera { yaw: 0.0, pitch: 0.0, velocity: Vec2::ZERO, air_height: 0.0, vertical_speed: 0.0, stance: Stance::Stand, eye_height: 1.8, jump_spent_standing: false, floor: 10.0, mantle: None, mantles: 0, sprint_suppressed: false, sliding: false, view_feet: 10.0, lean: 0.0, lean_offset: Vec2::ZERO };
        let eye = app.world_mut().spawn((Transform::from_xyz(3.0, 11.8, -4.0).with_rotation(Quat::from_rotation_y(0.5)), cam)).id();
        let body = app.world_mut().spawn((OwnBody, Transform::default(), Visibility::default())).id();
        app.update();
        let t = *app.world().get::<Transform>(body).unwrap();
        // Under the eye, a little back of it, at the height of the feet.
        assert!((t.translation.y - 10.0).abs() < 1e-5);
        let flat = Vec2::new(t.translation.x - 3.0, t.translation.z + 4.0);
        assert!((flat.length() - SET_BACK).abs() < 1e-4, "{flat}");
        // Back is the way the player is not facing.
        let facing = Vec2::new(-(0.5f32).sin(), -(0.5f32).cos());
        assert!(flat.dot(facing) < 0.0);
        assert!((t.scale.y - 1.8 / MODEL_EYE).abs() < 1e-5);

        // Crouched: shorter. Prone: not shown.
        {
            let mut e = app.world_mut().entity_mut(eye);
            let mut c = e.get_mut::<FpsCamera>().unwrap();
            c.eye_height = 1.2;
        }
        app.update();
        assert!(app.world().get::<Transform>(body).unwrap().scale.y < t.scale.y);
        {
            let mut e = app.world_mut().entity_mut(eye);
            e.get_mut::<FpsCamera>().unwrap().stance = Stance::Prone;
        }
        app.update();
        assert_eq!(*app.world().get::<Visibility>(body).unwrap(), Visibility::Hidden);
    }
}
