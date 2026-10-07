// Bevy loads JPG/PNG textures with a single mip level, so anything tiled across the ground
// shimmers into grain at a distance. Textures queued here get a full mip chain built on the
// CPU as soon as they finish loading. Averaging is done in linear light, and colour is
// weighted by alpha so alpha-cut foliage atlases don't bleed dark fringes into their edges.

use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

pub struct MipmapPlugin;

impl Plugin for MipmapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MipQueue>().add_systems(Update, build_queued_mips);
    }
}

#[derive(Resource, Default)]
pub struct MipQueue(pub Vec<Handle<Image>>);

fn build_queued_mips(mut queue: ResMut<MipQueue>, mut images: ResMut<Assets<Image>>) {
    queue.0.retain(|handle| {
        // Keep waiting until the image has loaded.
        let Some(image) = images.get(handle) else { return true };
        let srgb = match image.texture_descriptor.format {
            TextureFormat::Rgba8UnormSrgb => true,
            TextureFormat::Rgba8Unorm => false,
            _ => return false,
        };
        if image.texture_descriptor.mip_level_count > 1 || image.data.is_none() {
            return false;
        }
        let (w, h) = (image.texture_descriptor.size.width as usize, image.texture_descriptor.size.height as usize);
        let chain = build_chain(image.data.as_deref().unwrap_or(&[]), w, h, srgb);
        if let Some(mut image) = images.get_mut(handle) {
            image.texture_descriptor.mip_level_count = chain.1;
            image.data = Some(chain.0);
        }
        false
    });
}

fn to_linear(v: u8, srgb: bool) -> f32 {
    let f = v as f32 / 255.0;
    if srgb { f.powf(2.2) } else { f }
}

fn from_linear(f: f32, srgb: bool) -> u8 {
    let f = if srgb { f.max(0.0).powf(1.0 / 2.2) } else { f };
    (f * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

// Returns all mip levels concatenated (largest first) and how many there are.
pub(crate) fn build_chain(base: &[u8], width: usize, height: usize, srgb: bool) -> (Vec<u8>, u32) {
    let mut data = base.to_vec();
    let mut levels = 1;
    let (mut w, mut h) = (width, height);
    let mut prev = base.to_vec();
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                let (mut rgb, mut alpha) = ([0.0f32; 3], 0.0f32);
                let mut count = 0.0;
                for dy in 0..2 {
                    for dx in 0..2 {
                        let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                        let i = (sy * w + sx) * 4;
                        let a = prev[i + 3] as f32 / 255.0;
                        for c in 0..3 {
                            rgb[c] += to_linear(prev[i + c], srgb) * a;
                        }
                        alpha += a;
                        count += 1.0;
                    }
                }
                let o = (y * nw + x) * 4;
                for c in 0..3 {
                    next[o + c] = from_linear(if alpha > 0.0 { rgb[c] / alpha } else { 0.0 }, srgb);
                }
                next[o + 3] = ((alpha / count) * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
        data.extend_from_slice(&next);
        prev = next;
        (w, h) = (nw, nh);
        levels += 1;
    }
    (data, levels)
}
