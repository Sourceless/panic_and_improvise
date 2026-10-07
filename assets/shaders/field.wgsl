#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var soil_plough_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var soil_plough_smp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var soil_loam_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var soil_loam_smp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var meadow_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var meadow_smp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var pasture_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var pasture_smp: sampler;

// Field kinds; must match fill::FieldKind.
const PLOUGHED: i32 = 0;
const WHEAT: i32 = 1;
const BARLEY: i32 = 2;
const RAPESEED: i32 = 3;
const ROW_CROP: i32 = 4;
const MAIZE: i32 = 5;
const STUBBLE: i32 = 6;
const LEGUME: i32 = 7;
const HAY: i32 = 8;
const PASTURE: i32 = 9;
const ROUGH: i32 = 10;

const TAU: f32 = 6.2831853;

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

fn fbm2(p: vec2<f32>) -> f32 {
    return 0.5 * vnoise(p) + 0.25 * vnoise(p * 2.03) + 0.125 * vnoise(p * 4.1) + 0.0625 * vnoise(p * 8.3);
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

#ifdef VERTEX_COLORS
    let kind = i32(round(in.color.a * 16.0));
    let tint = in.color.rgb;
#else
    let kind = 0;
    let tint = vec3<f32>(1.0);
#endif
#ifdef VERTEX_UVS_A
    let rows = in.uv;
#else
    let rows = in.world_position.xz;
#endif

    let wp = in.world_position.xz;
    let dist = length(view.world_position - in.world_position.xyz);
    // Fine patterns (rows, furrows) would alias into noise at range, so they fade out and
    // are replaced by their average appearance.
    let fine = 1.0 - smoothstep(35.0, 180.0, dist);

    let soil_p = textureSample(soil_plough_tex, soil_plough_smp, wp / 3.0).rgb;
    let soil_l = textureSample(soil_loam_tex, soil_loam_smp, wp / 3.0).rgb;
    let meadow = textureSample(meadow_tex, meadow_smp, wp / 2.5).rgb;
    let lush = textureSample(pasture_tex, pasture_smp, wp / 2.5).rgb;
    let n_lo = fbm2(wp / 60.0);
    let n_mid = fbm2(wp / 11.0);
    let n_hi = vnoise(wp / 1.7);

    // Canopy micro-detail from the grass texture, normalised around 1.
    let detail = clamp(luma(lush) / 0.22, 0.55, 1.7);

    var col = tint;
    if kind == PLOUGHED {
        let ridge = 0.5 + 0.5 * cos(TAU * rows.x / 0.8);
        let shade = mix(0.82, 0.55 + 0.7 * ridge, fine);
        col = soil_p * tint * shade * (0.85 + 0.3 * n_lo);
    } else if kind == WHEAT || kind == BARLEY {
        // Tramlines: unsown tracks every 24 m, plus faint drill lines up close.
        let tram_mask = 1.0 - (1.0 - smoothstep(0.0, 0.03, abs(fract(rows.x / 24.0 + 0.5) - 0.5))) * 0.22;
        let drill = 1.0 - 0.10 * fine * smoothstep(0.4, 0.5, abs(fract(rows.x / 0.18) - 0.5));
        col = tint * detail * (0.88 + 0.24 * n_mid) * (0.94 + 0.12 * n_lo) * drill * tram_mask;
    } else if kind == RAPESEED {
        let flowers = smoothstep(0.45, 0.75, n_mid + 0.25 * n_hi);
        col = mix(tint * 0.78, tint * 1.08, flowers) * (0.92 + 0.16 * n_lo);
    } else if kind == ROW_CROP || kind == MAIZE {
        let spacing = select(0.9, 0.75, kind == MAIZE);
        let ridge = 0.5 + 0.5 * cos(TAU * rows.x / spacing);
        let cover_rows = smoothstep(0.15, 0.7, ridge);
        let avg_cover = select(0.55, 0.85, kind == MAIZE);
        let cover = mix(avg_cover, cover_rows, fine);
        let soil = soil_l * 0.55;
        col = mix(soil, tint * detail * (0.85 + 0.3 * n_mid), cover);
    } else if kind == STUBBLE {
        let lines = 1.0 - 0.15 * fine * smoothstep(0.35, 0.5, abs(fract(rows.x / 0.25) - 0.5));
        col = mix(soil_l * 0.9, tint * detail, 0.62) * lines * (0.88 + 0.24 * n_lo);
    } else if kind == LEGUME {
        col = tint * detail * (0.85 + 0.3 * n_mid) * (0.94 + 0.12 * n_lo);
    } else if kind == HAY {
        // Mown stripes: alternating bands across the field.
        let band = select(0.92, 1.08, fract(rows.x / 9.0) > 0.5);
        col = meadow * tint * band * (0.9 + 0.2 * n_lo) * 1.9;
    } else if kind == PASTURE {
        col = lush * tint * (0.88 + 0.24 * n_mid) * (0.92 + 0.16 * n_lo) * 1.7;
    } else {
        // Rough grazing: dry tussocky grass with patches of bare ground.
        let bare = smoothstep(0.62, 0.8, n_mid + 0.2 * n_hi);
        col = mix(meadow * tint * 2.0, soil_l * 0.8, bare * 0.5) * (0.85 + 0.3 * n_lo);
    }

    pbr_input.material.base_color = vec4<f32>(col, 1.0);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
