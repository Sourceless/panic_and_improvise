// Water: a transparent PBR surface whose colour, clarity and shoreline foam come from the
// water's depth, stored per vertex in the colour's red channel (metres), and whose surface
// is rippled by scrolling noise in assets/shaders/water.wgsl. The lighting itself is the
// standard PBR path, so the sun glints on the ripples and the sky reflects in the surface.

use bevy::asset::RenderAssetUsages;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};

use crate::map::{TerrainMap, CELL, HALF_SIZE};
use bevy::shader::ShaderRef;

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExtension>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct WaterExtension {
    /// The terrain's heights, one texel per grid vertex, so the shader can work out how deep
    /// the water is at every pixel instead of interpolating a per-vertex guess.
    #[texture(100, sample_type = "float", filterable = false)]
    pub bed: Handle<Image>,
    /// x: half the map size, y: cell size, z: vertices per side.
    #[uniform(101)]
    pub map_info: Vec4,
    /// x: how far to turn the waves from their authored direction (radians), y: sea roughness.
    #[uniform(102)]
    pub wind: Vec4,
}

impl MaterialExtension for WaterExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/water.wgsl".into()
    }
}

/// The terrain's heights as an f32 texture, row by row, for [`WaterExtension::bed`].
pub fn bed_texture(map: &TerrainMap) -> Image {
    let n = map.grid_size();
    let mut data = Vec::with_capacity(n * n * 4);
    for iz in 0..n {
        for ix in 0..n {
            data.extend_from_slice(&map.vertex_height(ix, iz).to_le_bytes());
        }
    }
    Image::new(
        Extent3d { width: n as u32, height: n as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::R32Float,
        RenderAssetUsages::RENDER_WORLD,
    )
}

pub fn water_material(map: &TerrainMap, images: &mut Assets<Image>) -> WaterMaterial {
    let n = map.grid_size();
    WaterMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.1,
            reflectance: 0.6,
            // Seen from below the surface is as good as invisible, and never worth culling.
            cull_mode: None,
            ..default()
        },
        extension: WaterExtension {
            bed: images.add(bed_texture(map)),
            map_info: Vec4::new(HALF_SIZE, CELL, n as f32, 0.0),
            wind: Vec4::new(0.0, 1.0, 0.0, 0.0),
        },
    }
}
