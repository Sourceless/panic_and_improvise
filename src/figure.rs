//! A person standing in the world: the model for the other players. A cap and a jacket, a pack, boots, a gun held
//! in both hands; the legs swing when the figure walks, the faster the more, and stand still when it does not.
//! It faces -Z with its feet at its origin, like the other models.

use bevy::prelude::*;

use crate::gun_models::Finish;
use crate::weapons::WeaponKind;

/// How many different cap colours there are; a player's is picked by their number.
pub const CAPS: usize = 8;

/// The materials and the box that a figure is made of.
#[derive(Resource)]
pub struct FigureLook {
    cuboid: Handle<Mesh>,
    skin: Handle<StandardMaterial>,
    jacket: Handle<StandardMaterial>,
    trousers: Handle<StandardMaterial>,
    boots: Handle<StandardMaterial>,
    pack: Handle<StandardMaterial>,
    caps: Vec<Handle<StandardMaterial>>,
}

impl FigureLook {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> FigureLook {
        let mut paint = |rgb: [f32; 3], roughness: f32| materials.add(StandardMaterial { base_color: Color::srgb(rgb[0], rgb[1], rgb[2]), perceptual_roughness: roughness, ..default() });
        FigureLook {
            cuboid: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            skin: paint([0.86, 0.68, 0.55], 0.8),
            jacket: paint([0.27, 0.32, 0.2], 1.0),
            trousers: paint([0.21, 0.22, 0.25], 1.0),
            boots: paint([0.08, 0.07, 0.06], 0.7),
            pack: paint([0.3, 0.26, 0.17], 1.0),
            caps: CAP_COLOURS.iter().map(|&rgb| paint(rgb, 0.9)).collect(),
        }
    }
}

/// Red, blue, yellow, orange, white, purple, teal and pink: easy to tell apart at a distance.
const CAP_COLOURS: [[f32; 3]; CAPS] = [[0.8, 0.15, 0.12], [0.15, 0.3, 0.8], [0.9, 0.78, 0.15], [0.9, 0.45, 0.1], [0.9, 0.9, 0.88], [0.5, 0.2, 0.7], [0.1, 0.6, 0.6], [0.9, 0.45, 0.65]];

/// Which part of the body swings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Joint {
    LeftLeg,
    RightLeg,
    LeftArm,
    RightArm,
}

/// A part that swings about its top, with how it hangs when the figure stands still.
#[derive(Component)]
pub struct Limb {
    pub joint: Joint,
    rest: Quat,
}

/// How far along its stride a figure is, and how fast it is going: worked out from how it moves.
#[derive(Component, Default)]
pub struct Gait {
    pub phase: f32,
    pub speed: f32,
    previous: Option<Vec3>,
}

/// How far the legs swing at most, radians, and how much of that the arms do (they hold a gun).
const LEG_SWING: f32 = 0.8;
const ARM_SWING: f32 = 0.06;
/// Radians of stride per metre walked.
const STRIDE: f32 = 2.6;
/// The speed at which the swing is the whole of it: a run.
const FULL_SWING_SPEED: f32 = 5.0;

/// How far forward each part swings, for a figure `phase` radians into its stride at `speed` metres a second:
/// the legs in turn, the arms a little.
pub fn swing(joint: Joint, phase: f32, speed: f32) -> f32 {
    let amount = (speed / FULL_SWING_SPEED).clamp(0.0, 1.0);
    // A slow walk is a short step.
    let leg = phase.sin() * LEG_SWING * amount.sqrt();
    match joint {
        Joint::LeftLeg => leg,
        Joint::RightLeg => -leg,
        Joint::LeftArm => -phase.sin() * ARM_SWING * amount,
        Joint::RightArm => phase.sin() * ARM_SWING * amount,
    }
}

/// Builds a person with their feet at the parent's origin. `cap` picks the cap colour; `gun` is the model of what
/// they carry, if anything (the parts and what each is drawn with).
pub fn spawn_figure(parent: &mut ChildSpawnerCommands, look: &FigureLook, cap: usize, gun: &[(Handle<Mesh>, Handle<StandardMaterial>)]) {
    let part = |centre: Vec3, size: Vec3, material: &Handle<StandardMaterial>| (Mesh3d(look.cuboid.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(centre).with_scale(size));
    // The trunk, the head, the pack on the back, the cap.
    parent.spawn(part(Vec3::new(0.0, 1.24, 0.0), Vec3::new(0.42, 0.58, 0.24), &look.jacket));
    parent.spawn(part(Vec3::new(0.0, 0.97, 0.0), Vec3::new(0.38, 0.08, 0.22), &look.trousers));
    parent.spawn(part(Vec3::new(0.0, 1.67, 0.0), Vec3::new(0.2, 0.23, 0.22), &look.skin));
    parent.spawn(part(Vec3::new(0.0, 1.8, 0.0), Vec3::new(0.23, 0.07, 0.25), &look.caps[cap % CAPS]));
    parent.spawn(part(Vec3::new(0.0, 1.78, -0.15), Vec3::new(0.17, 0.02, 0.1), &look.caps[cap % CAPS]));
    parent.spawn(part(Vec3::new(0.0, 1.28, 0.19), Vec3::new(0.32, 0.42, 0.16), &look.pack));
    // Legs, swinging from the hip, with boots at the bottom.
    for (joint, side) in [(Joint::LeftLeg, -1.0), (Joint::RightLeg, 1.0)] {
        parent
            .spawn((Limb { joint, rest: Quat::IDENTITY }, Transform::from_xyz(side * 0.1, 0.93, 0.0), Visibility::default()))
            .with_children(|leg| {
                leg.spawn(part(Vec3::new(0.0, -0.4, 0.0), Vec3::new(0.17, 0.8, 0.2), &look.trousers));
                leg.spawn(part(Vec3::new(0.0, -0.87, -0.04), Vec3::new(0.18, 0.12, 0.3), &look.boots));
            });
    }
    // Arms, brought forward to hold the gun: the right at the grip, the left under the front of it.
    for (joint, side) in [(Joint::LeftArm, -1.0), (Joint::RightArm, 1.0)] {
        let forward = if side < 0.0 { 1.35 } else { 1.05 };
        let rest = Quat::from_rotation_y(-side * 0.28) * Quat::from_rotation_x(forward);
        parent
            .spawn((Limb { joint, rest }, Transform::from_xyz(side * 0.27, 1.47, 0.0).with_rotation(rest), Visibility::default()))
            .with_children(|arm| {
                arm.spawn(part(Vec3::new(0.0, -0.26, 0.0), Vec3::new(0.11, 0.52, 0.12), &look.jacket));
                arm.spawn(part(Vec3::new(0.0, -0.55, 0.0), Vec3::new(0.09, 0.09, 0.09), &look.skin));
            });
    }
    // The gun, across the front of the body, its muzzle forward.
    if !gun.is_empty() {
        parent.spawn((Transform::from_xyz(0.1, 1.3, -0.12), Visibility::default())).with_children(|g| {
            for (mesh, material) in gun {
                g.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default()));
            }
        });
    }
}

/// The meshes and materials of `kind`'s model, for `spawn_figure`.
pub fn gun_parts(assets: &crate::weapon::WeaponAssets, kind: WeaponKind) -> Vec<(Handle<Mesh>, Handle<StandardMaterial>)> {
    assets.models[&kind].parts.iter().map(|(finish, mesh): &(Finish, Handle<Mesh>)| (mesh.clone(), assets.finishes[finish].clone())).collect()
}

/// Works out how fast each figure is going from how far it has moved, and carries it along its stride.
pub fn measure_gait(time: Res<Time>, mut figures: Query<(&Transform, &mut Gait)>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    for (transform, mut gait) in &mut figures {
        let here = transform.translation;
        let moved = gait.previous.map_or(0.0, |before| Vec2::new(here.x - before.x, here.z - before.z).length());
        gait.previous = Some(here);
        // Eased, so that the steps of a figure that is being glided to its place do not make it jerk.
        let speed = moved / dt;
        gait.speed += (speed - gait.speed) * (1.0 - (-10.0 * dt).exp());
        if gait.speed < 0.05 {
            gait.speed = 0.0;
        }
        gait.phase = (gait.phase + moved * STRIDE).rem_euclid(std::f32::consts::TAU);
    }
}

/// Swings the limbs of each figure by its gait.
pub fn swing_limbs(gaits: Query<&Gait>, mut limbs: Query<(&ChildOf, &Limb, &mut Transform)>) {
    for (parent, limb, mut transform) in &mut limbs {
        let Ok(gait) = gaits.get(parent.parent()) else { continue };
        transform.rotation = limb.rest * Quat::from_rotation_x(-swing(limb.joint, gait.phase, gait.speed));
    }
}

pub struct FigurePlugin;

impl Plugin for FigurePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_look).add_systems(Update, (measure_gait, swing_limbs).chain());
    }
}

fn make_look(mut commands: Commands, meshes: Option<ResMut<Assets<Mesh>>>, materials: Option<ResMut<Assets<StandardMaterial>>>) {
    let (Some(mut meshes), Some(mut materials)) = (meshes, materials) else { return };
    commands.insert_resource(FigureLook::new(&mut meshes, &mut materials));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_figure_that_stands_still_does_not_swing_and_one_that_runs_does_with_its_legs_in_turn() {
        for joint in [Joint::LeftLeg, Joint::RightLeg, Joint::LeftArm, Joint::RightArm] {
            assert_eq!(swing(joint, 1.3, 0.0), 0.0);
        }
        let run = 6.0;
        let (left, right) = (swing(Joint::LeftLeg, 1.0, run), swing(Joint::RightLeg, 1.0, run));
        assert!(left.abs() > 0.5 && (left + right).abs() < 1e-6, "legs in turn: {left} {right}");
        assert!(swing(Joint::LeftLeg, 1.0, 1.0).abs() < left.abs(), "a walk is a shorter step than a run");
        assert!(swing(Joint::LeftArm, 1.0, run).abs() < left.abs() * 0.2, "the arms hold the gun");
    }

    /// A world with a figure at a place, which moves.
    fn app_with_a_figure() -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<Time>().add_systems(Update, (measure_gait, swing_limbs).chain());
        let figure = app.world_mut().spawn((Transform::default(), Gait::default(), Visibility::default())).id();
        let mut commands_world = app.world_mut().commands();
        commands_world.entity(figure).with_children(|f| {
            f.spawn((Limb { joint: Joint::LeftLeg, rest: Quat::IDENTITY }, Transform::default()));
            f.spawn((Limb { joint: Joint::RightLeg, rest: Quat::IDENTITY }, Transform::default()));
        });
        app.world_mut().flush();
        (app, figure)
    }

    fn step(app: &mut App, dt: f32) {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(dt));
        app.update();
    }

    fn legs(app: &mut App) -> Vec<Quat> {
        let mut query = app.world_mut().query::<(&Limb, &Transform)>();
        query.iter(app.world()).map(|(_, t)| t.rotation).collect()
    }

    #[test]
    fn a_figure_walks_when_it_moves_and_stops_when_it_stops() {
        let (mut app, figure) = app_with_a_figure();
        step(&mut app, 0.05);
        assert!(legs(&mut app).iter().all(|q| q.angle_between(Quat::IDENTITY) < 1e-4), "standing");
        let mut most = 0.0f32;
        for i in 0..60 {
            app.world_mut().get_mut::<Transform>(figure).unwrap().translation.z -= 4.0 * 0.05;
            step(&mut app, 0.05);
            if i > 20 {
                most = most.max(legs(&mut app).iter().map(|q| q.angle_between(Quat::IDENTITY)).fold(0.0, f32::max));
            }
        }
        assert!(most > 0.4, "swung {most} rad at 4 m/s");
        for _ in 0..80 {
            step(&mut app, 0.05);
        }
        assert!(legs(&mut app).iter().all(|q| q.angle_between(Quat::IDENTITY) < 1e-3), "still again");
    }

    #[test]
    fn a_figure_has_a_person_sized_body_with_a_head_at_about_the_height_of_one() {
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let look = FigureLook::new(&mut meshes, &mut materials);
        let mut world = World::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, &world);
            commands.spawn(Transform::default()).with_children(|f| spawn_figure(f, &look, 3, &[]));
        }
        queue.apply(&mut world);
        let mut top = f32::MIN;
        let mut query = world.query::<(&Mesh3d, &Transform, Option<&ChildOf>)>();
        let mut parts = 0;
        for (_, t, _) in query.iter(&world) {
            top = top.max(t.translation.y + t.scale.y * 0.5);
            parts += 1;
        }
        assert!(parts >= 10, "{parts} parts");
        assert!((1.75..1.9).contains(&top), "{top} m tall (the cap on the head)");
    }
}
