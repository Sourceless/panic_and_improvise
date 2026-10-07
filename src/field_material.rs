// Material for field fill: real soil and grass textures with a crop / land-use pattern laid
// over them by assets/shaders/field.wgsl. Per-vertex data drives it: colour rgb is the
// field's tint, colour alpha is the field kind (see fill::FieldKind), and uv_0 holds the
// vertex's position in the field's own row-aligned frame, in metres (x across the rows,
// y along them).

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub type FieldMaterial = ExtendedMaterial<StandardMaterial, FieldExtension>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct FieldExtension {
    /// The shared ground texture arrays (see ground_textures), colour and normal maps.
    #[texture(100, dimension = "2d_array")]
    #[sampler(101)]
    pub diffuse: Handle<Image>,
    #[texture(102, dimension = "2d_array")]
    #[sampler(103)]
    pub normal: Handle<Image>,
}

impl MaterialExtension for FieldExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/field.wgsl".into()
    }
}
