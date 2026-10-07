#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
}

// Every kind of ground is one layer of these two arrays (colour, normal map). The layer
// numbers must match ground_textures::layer, and the weights arriving per vertex are in the
// same order: grass, dirt, stone, sand, gravel, leaf litter, needle litter, mud.
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var ground_diffuse: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var ground_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var ground_normal: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var normal_sampler: sampler;

const TILE_METRES: f32 = 6.0;
const GRASS_TINT: vec3<f32> = vec3<f32>(0.75, 1.2, 0.55);
const CONTOUR_SPACING: f32 = 5.0;
const CONTOUR_HALF_WIDTH: f32 = 0.12;
// How strongly each layer's normal map tilts the surface, relative to the map as authored.
const NORMAL_STRENGTH: f32 = 0.9;

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

// A layer sampled twice at unrelated scales and blended by noise, which hides the repeat
// that a single tiling would show across a large area.
fn untiled(layer: i32, wp: vec2<f32>, mixer: f32) -> vec3<f32> {
    let a = textureSample(ground_diffuse, ground_sampler, wp / TILE_METRES, layer).rgb;
    let b = textureSample(ground_diffuse, ground_sampler, wp / (TILE_METRES * 2.37) + vec2<f32>(0.37, 0.71), layer).rgb;
    return mix(a, b, mixer);
}

// The tilt (x, y) a layer's normal map gives the surface. Green is "up" in the image, which
// for ground texture coordinates (u = world x, v = world z) is toward -z.
fn tilt(layer: i32, wp: vec2<f32>) -> vec2<f32> {
    return textureSample(ground_normal, normal_sampler, wp / TILE_METRES, layer).xy * 2.0 - 1.0;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let wp = in.world_position.xz;
    let mixer = 0.25 + 0.5 * vnoise(wp / 37.0);

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
    // Weights in layer order.
    let w = array<f32, 8>(wa.r, wa.g, wa.b, wa.a, wb.x, wb.y, wc.x, wc.y);

    var color = untiled(0, wp, mixer) * GRASS_TINT * w[0]
        + untiled(1, wp, mixer) * w[1]
        + untiled(2, wp, mixer) * w[2]
        + untiled(3, wp, mixer) * w[3]
        + untiled(4, wp, mixer) * w[4]
        + untiled(5, wp, mixer) * w[5]
        + untiled(6, wp, mixer) * w[6]
        + untiled(7, wp, mixer) * 0.75 * w[7];

    // Normal maps, blended by the same weights, tilting the (smooth) terrain normal. Gentler on
    // grass and sand, stronger on rock and gravel, and faded out with distance, where it would
    // only shimmer.
    var t = tilt(0, wp) * (0.5 * w[0])
        + tilt(1, wp) * (1.0 * w[1])
        + tilt(2, wp) * (1.2 * w[2])
        + tilt(3, wp) * (0.7 * w[3])
        + tilt(4, wp) * (1.2 * w[4])
        + tilt(5, wp) * (1.0 * w[5])
        + tilt(6, wp) * (1.0 * w[6])
        + tilt(7, wp) * (1.0 * w[7]);
    let dist = length(view.world_position - in.world_position.xyz);
    let fade = 1.0 - smoothstep(40.0, 220.0, dist);
    let n0 = normalize(pbr_input.N);
    let tangent = normalize(vec3<f32>(1.0, 0.0, 0.0) - n0 * n0.x);
    let bitangent = normalize(vec3<f32>(0.0, 0.0, -1.0) + n0 * n0.z);
    let n = normalize(n0 + (tangent * t.x + bitangent * t.y) * fade * NORMAL_STRENGTH);
    pbr_input.N = n;

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
