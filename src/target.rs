use bevy::prelude::*;

pub const DUMMY_HALF_EXTENTS: Vec3 = Vec3::new(0.35, 1.0, 0.35);
const DUMMY_MAX_HEALTH: f32 = 100.0;
const RESPAWN_DELAY: f32 = 2.0;
const HIT_FLASH_TIME: f32 = 0.12;

pub struct TargetPlugin;

impl Plugin for TargetPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_dummy)
            .add_systems(Update, update_dummy);
    }
}

#[derive(Component)]
pub struct TargetDummy {
    pub health: f32,
    pub hits: u32,
    pub body_material: Handle<StandardMaterial>,
    flash: f32,
    respawn: f32,
}

impl TargetDummy {
    pub fn take_hit(&mut self, damage: f32) {
        self.health = (self.health - damage).max(0.0);
        self.hits += 1;
        self.flash = HIT_FLASH_TIME;
        if self.health <= 0.0 {
            self.respawn = RESPAWN_DELAY;
        }
    }
}

pub fn dummy_aabb(base: Vec3) -> (Vec3, Vec3) {
    let center = base + Vec3::Y * DUMMY_HALF_EXTENTS.y;
    (center - DUMMY_HALF_EXTENTS, center + DUMMY_HALF_EXTENTS)
}

pub fn spawn_dummy(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.75, 0.6),
        ..default()
    });
    let body_mesh = meshes.add(Cuboid::new(0.7, 1.6, 0.45));
    let head_mesh = meshes.add(Sphere::new(0.2));

    commands
        .spawn((
            Transform::from_xyz(0.0, 0.0, -15.0),
            Visibility::default(),
            TargetDummy {
                health: DUMMY_MAX_HEALTH,
                hits: 0,
                body_material: material.clone(),
                flash: 0.0,
                respawn: 0.0,
            },
        ))
        .with_children(|dummy| {
            dummy.spawn((
                Mesh3d(body_mesh),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(0.0, 0.8, 0.0),
            ));
            dummy.spawn((
                Mesh3d(head_mesh),
                MeshMaterial3d(material),
                Transform::from_xyz(0.0, 1.8, 0.0),
            ));
        });
}

fn update_dummy(
    time: Res<Time>,
    mut dummies: Query<&mut TargetDummy>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    for mut dummy in &mut dummies {
        if dummy.health <= 0.0 {
            dummy.respawn -= dt;
            if dummy.respawn <= 0.0 {
                dummy.health = DUMMY_MAX_HEALTH;
            }
        }
        dummy.flash = (dummy.flash - dt).max(0.0);

        let color = if dummy.health <= 0.0 {
            Color::srgb(0.15, 0.15, 0.15)
        } else if dummy.flash > 0.0 {
            Color::srgb(1.0, 0.25, 0.2)
        } else {
            Color::srgb(0.85, 0.75, 0.6)
        };

        let needs_update = materials
            .get(&dummy.body_material)
            .is_some_and(|m| m.base_color != color);
        if needs_update {
            if let Some(mut material) = materials.get_mut(&dummy.body_material) {
                material.base_color = color;
            }
        }
    }
}
