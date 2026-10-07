// A standard PBR material with wind: a vertex shader (assets/shaders/wind.wgsl) that sways
// vertices more the higher they are on the mesh, with a slow gust field rolling across the
// landscape and a fine flutter on leaves. All the motion happens on the GPU, so it costs
// nothing on the CPU and meshes stay shared and batched.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub type WindMaterial = ExtendedMaterial<StandardMaterial, WindExtension>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WindExtension {
    /// x: sway at the top of the object, in metres; y: the height (in mesh units) at which
    /// that full sway is reached; z: leaf flutter, in metres.
    #[uniform(100)]
    pub params: Vec4,
    /// The world's wind (see wind.rs): x, y the direction it blows along, z its strength
    /// relative to the breeze the sway was tuned at. Kept up to date by the wind plugin.
    #[uniform(101)]
    pub wind: Vec4,
}

impl MaterialExtension for WindExtension {
    fn vertex_shader() -> ShaderRef {
        "shaders/wind.wgsl".into()
    }
}

impl WindExtension {
    pub fn new(sway: f32, height: f32, flutter: f32) -> Self {
        WindExtension { params: Vec4::new(sway, height, flutter, 0.0), wind: Vec4::new(0.94, 0.34, 1.0, 0.0) }
    }
}
