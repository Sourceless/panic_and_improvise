// The material for roads: a surface texture (tarmac or gravel) with everything else about a
// road drawn by assets/shaders/road.wgsl: worn wheel tracks, soft edges, and the white lines
// of a British road (a broken centre line and continuous edge lines).
//
// The mesh carries what the shader needs: UV 0 tiles the surface texture over the ground,
// UV 1 is (metres across from the centre line, metres along the road), and the vertex colour is
// (lines painted here, half width / 10 m, is a track, 1) - see roads::road_vertex_data.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub type RoadMaterial = ExtendedMaterial<StandardMaterial, RoadExtension>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct RoadExtension {
    /// The road's surface: tarmac for main roads, gravel for tracks.
    #[texture(100)]
    #[sampler(101)]
    pub surface: Handle<Image>,
}

impl MaterialExtension for RoadExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/road.wgsl".into()
    }
}
