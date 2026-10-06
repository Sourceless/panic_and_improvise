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

const TILE_METRES: f32 = 6.0;
const GRASS_TINT: vec3<f32> = vec3<f32>(0.75, 1.2, 0.55);
const CONTOUR_SPACING: f32 = 5.0;
const CONTOUR_HALF_WIDTH: f32 = 0.12;

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let uv = in.world_position.xz / TILE_METRES;
    let grass = textureSample(grass_texture, grass_sampler, uv).rgb * GRASS_TINT;
    let dirt = textureSample(dirt_texture, dirt_sampler, uv).rgb;
    let stone = textureSample(stone_texture, stone_sampler, uv).rgb;

#ifdef VERTEX_COLORS
    let weights = in.color;
#else
    let weights = vec4<f32>(1.0, 0.0, 0.0, 1.0);
#endif
    var color = grass * weights.r + dirt * weights.g + stone * weights.b;

    let height_from_contour = abs(fract(in.world_position.y / CONTOUR_SPACING + 0.5) - 0.5) * CONTOUR_SPACING;
    let on_contour = 1.0 - smoothstep(0.0, CONTOUR_HALF_WIDTH, height_from_contour);
    color = color * (1.0 - 0.35 * on_contour);

    pbr_input.material.base_color = vec4<f32>(color, 1.0);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
