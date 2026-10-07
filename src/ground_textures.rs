// The ground's textures, packed into two texture arrays (colour and normal map, one layer per
// kind of ground) that the terrain and field shaders share.
//
// Arrays rather than separate textures because a shader can only bind so many textures; one
// array of ten layers is a single binding, so there is room for a normal map for each layer
// and for sampling any layer by index. The layers are loaded as ordinary images, then built
// into arrays with full mip chains once they have all arrived.

use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
use bevy::asset::RenderAssetUsages;

/// Layer names, in layer order. Each has `<name>.jpg` and `<name>_n.jpg` under textures/pbr.
pub const LAYERS: [&str; 10] = [
    "pasture",
    "dirt",
    "stone",
    "sand",
    "gravel",
    "forest_broadleaf",
    "forest_conifer",
    "soil_plough",
    "soil_loam",
    "meadow",
];

/// Layer indices, for the shaders (which must agree with `LAYERS`).
pub mod layer {
    pub const GRASS: i32 = 0;
    pub const DIRT: i32 = 1;
    pub const STONE: i32 = 2;
    pub const SAND: i32 = 3;
    pub const GRAVEL: i32 = 4;
    pub const LITTER: i32 = 5;
    pub const NEEDLES: i32 = 6;
    pub const MUD: i32 = 7;
    pub const LOAM: i32 = 8;
    pub const MEADOW: i32 = 9;
}

const SIZE: u32 = 1024;

#[derive(Resource, Clone)]
pub struct GroundArrays {
    pub diffuse: Handle<Image>,
    pub normal: Handle<Image>,
}

#[derive(Resource)]
struct Pending {
    diffuse: Vec<Handle<Image>>,
    normal: Vec<Handle<Image>>,
}

pub struct GroundTexturesPlugin;

impl Plugin for GroundTexturesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, assemble_arrays);
    }
}

/// Starts loading every layer and returns the (still empty) array handles the materials use.
/// They fill in a few frames later, when the layers have loaded.
pub fn start_loading(commands: &mut Commands, asset_server: &AssetServer, images: &mut Assets<Image>) -> GroundArrays {
    let load = |path: String, srgb: bool| -> Handle<Image> {
        asset_server
            .load_builder()
            .with_settings(move |settings: &mut ImageLoaderSettings| {
                settings.is_srgb = srgb;
                settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    ..ImageSamplerDescriptor::linear()
                });
            })
            .load(path)
    };
    let diffuse = LAYERS.iter().map(|n| load(format!("textures/pbr/{n}.jpg"), true)).collect();
    let normal = LAYERS.iter().map(|n| load(format!("textures/pbr/{n}_n.jpg"), false)).collect();
    let arrays = GroundArrays { diffuse: images.reserve_handle(), normal: images.reserve_handle() };
    commands.insert_resource(Pending { diffuse, normal });
    commands.insert_resource(arrays.clone());
    arrays
}

fn assemble_arrays(mut commands: Commands, pending: Option<Res<Pending>>, arrays: Option<Res<GroundArrays>>, mut images: ResMut<Assets<Image>>) {
    let (Some(pending), Some(arrays)) = (pending, arrays) else { return };
    let ready = |handles: &[Handle<Image>]| handles.iter().all(|h| images.get(h).is_some());
    if !ready(&pending.diffuse) || !ready(&pending.normal) {
        return;
    }
    let diffuse = build_array(&images, &pending.diffuse, true);
    let normal = build_array(&images, &pending.normal, false);
    let _ = images.insert(&arrays.diffuse, diffuse);
    let _ = images.insert(&arrays.normal, normal);
    commands.remove_resource::<Pending>();
}

fn build_array(images: &Assets<Image>, layers: &[Handle<Image>], srgb: bool) -> Image {
    let mut data = Vec::new();
    let mut mips = 1;
    for handle in layers {
        let layer = images.get(handle).expect("checked loaded");
        let size = layer.texture_descriptor.size;
        assert_eq!((size.width, size.height), (SIZE, SIZE), "ground layers must be {SIZE}x{SIZE}");
        let (chain, levels) = crate::mipmaps::build_chain(layer.data.as_deref().unwrap_or(&[]), SIZE as usize, SIZE as usize, srgb);
        mips = levels;
        data.extend_from_slice(&chain);
    }
    let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
    // Image::new wants exactly the base level's worth of data; the full chain goes in after.
    let base = vec![0u8; (SIZE * SIZE * 4) as usize * layers.len()];
    let mut image = Image::new(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: layers.len() as u32 },
        TextureDimension::D2,
        base,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = mips;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        anisotropy_clamp: 8,
        ..ImageSamplerDescriptor::linear()
    });
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image
}
