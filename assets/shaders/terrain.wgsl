#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var grass_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var grass_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var dirt_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var dirt_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var stone_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var stone_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var sand_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var sand_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var gravel_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var gravel_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var litter_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(111) var litter_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(112) var needles_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(113) var needles_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(114) var mud_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(115) var mud_sampler: sampler;

const TILE_METRES: f32 = 6.0;
const GRASS_TINT: vec3<f32> = vec3<f32>(0.75, 1.2, 0.55);
const CONTOUR_SPACING: f32 = 5.0;
const CONTOUR_HALF_WIDTH: f32 = 0.12;

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 456.21));
    q += dot(q, q + 45.32);
    return fract(q.x * q.y);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash21(i), hash21(i + vec2<f32>(1.0, 0.0)), u.x),
        mix(hash21(i + vec2<f32>(0.0, 1.0)), hash21(i + vec2<f32>(1.0, 1.0)), u.x),
        u.y,
    );
}

// A texture sampled twice at unrelated scales and blended by noise, which hides the repeat
// that a single tiling would show across a large area.
fn untiled(tex: texture_2d<f32>, smp: sampler, wp: vec2<f32>, mixer: f32) -> vec3<f32> {
    let a = textureSample(tex, smp, wp / TILE_METRES).rgb;
    let b = textureSample(tex, smp, wp / (TILE_METRES * 2.37) + vec2<f32>(0.37, 0.71)).rgb;
    return mix(a, b, mixer);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let wp = in.world_position.xz;
    let mixer = 0.25 + 0.5 * vnoise(wp / 37.0);

    let grass = untiled(grass_texture, grass_sampler, wp, mixer) * GRASS_TINT;
    let dirt = untiled(dirt_texture, dirt_sampler, wp, mixer);
    let stone = untiled(stone_texture, stone_sampler, wp, mixer);
    let sand = untiled(sand_texture, sand_sampler, wp, mixer);
    let gravel = untiled(gravel_texture, gravel_sampler, wp, mixer);
    let litter = untiled(litter_texture, litter_sampler, wp, mixer);
    let needles = untiled(needles_texture, needles_sampler, wp, mixer);
    let mud = untiled(mud_texture, mud_sampler, wp, mixer) * 0.75;

#ifdef VERTEX_COLORS
    let wa = in.color;
#else
    let wa = vec4<f32>(1.0, 0.0, 0.0, 0.0);
#endif
#ifdef VERTEX_UVS_A
    let wb = in.uv;
#else
    let wb = vec2<f32>(0.0);
#endif
#ifdef VERTEX_UVS_B
    let wc = in.uv_b;
#else
    let wc = vec2<f32>(0.0);
#endif
    var color = grass * wa.r + dirt * wa.g + stone * wa.b + sand * wa.a
        + gravel * wb.x + litter * wb.y + needles * wc.x + mud * wc.y;

    // Broad patchiness so no large area is a uniform tone.
    let macro_tone = 0.84 + 0.32 * (0.6 * vnoise(wp / 95.0) + 0.4 * vnoise(wp / 23.0));
    color = color * macro_tone;

    let height_from_contour = abs(fract(in.world_position.y / CONTOUR_SPACING + 0.5) - 0.5) * CONTOUR_SPACING;
    // No contour lines on the sea floor, where they show through the water as a grid.
    let on_contour = (1.0 - smoothstep(0.0, CONTOUR_HALF_WIDTH, height_from_contour)) * step(0.0, in.world_position.y);
    color = color * (1.0 - 0.35 * on_contour);

    pbr_input.material.base_color = vec4<f32>(color, 1.0);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
