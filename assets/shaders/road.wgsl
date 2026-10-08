// Roads. The mesh gives UV 0 (the surface texture tiled over the ground), UV 1 = (metres across
// from the centre line, metres along the road), and a vertex colour of
// (lines painted here, half width / 10 m, is a track, edge line runs here). Tarmac gets worn wheel tracks, patched
// repairs, crumbling edges and the white lines of a British road, drawn exactly rather than
// from a texture: a broken centre line (3 m marks, 6 m gaps, TSRGD diagram 1008.1) and
// continuous edge lines, each 100 mm wide. Tracks get gravel, two ruts and grass between them; footpaths
// get packed earth with grass at the edges.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var surface_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var surface_sampler: sampler;

const MARK_LENGTH: f32 = 3.0;
const MARK_PERIOD: f32 = 9.0;
const LINE_HALF_WIDTH: f32 = 0.05;
// How far in from the road's edge the edge line runs, to its centre.
const EDGE_LINE_INSET: f32 = 0.30;

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

// How much of this pixel a band of half width `half_w` centred where `x` is zero covers. Using
// the pixel's own size keeps a thin line crisp up close and fading smoothly to nothing far away
// instead of shimmering. (Derivatives, so only call this in uniform control flow.)
fn cover(x: f32, half_w: f32) -> f32 {
    let aa = max(fwidth(x), 1e-4);
    return clamp((half_w - abs(x)) / aa + 0.5, 0.0, 1.0);
}

// The same for marks `MARK_LENGTH` long, once every `MARK_PERIOD`, along the road.
fn dashes(along: f32) -> f32 {
    let aa = max(fwidth(along), 1e-4);
    let phase = along - floor(along / MARK_PERIOD) * MARK_PERIOD;
    return clamp(phase / aa + 0.5, 0.0, 1.0) * clamp((MARK_LENGTH - phase) / aa + 0.5, 0.0, 1.0);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let across = in.uv_b.x;
    let along = in.uv_b.y;
    let lane = abs(across);
    let hw = in.color.g * 10.0;
    let painted = step(0.5, in.color.r);
    let track = in.color.b;
    let edge_painted = step(0.5, in.color.a);
    let wp = in.world_position.xz;

    // Everything that needs screen-space derivatives, before any branching.
    let surface = textureSample(surface_tex, surface_sampler, in.uv).rgb;
    let centre_line = cover(across, LINE_HALF_WIDTH) * dashes(along);
    let edge_line = cover(lane - (hw - EDGE_LINE_INSET), LINE_HALF_WIDTH);
    // Wheel paths run in each lane, a little either side of its middle; ruts on a track.
    let wheels = max(cover(lane - hw * 0.5 - 0.82, 0.22), cover(lane - hw * 0.5 + 0.82, 0.22));
    let ruts = cover(lane - 0.95, 0.28);

    let patches = fbm2(wp / 9.0);
    let grain = fbm2(wp / 1.3);
    // 0 well inside the road, 1 at its very edge.
    let edge = smoothstep(hw - 0.6, hw, lane);

    var col: vec3<f32>;
    var roughness = 0.92;
    if track > 0.25 && track < 0.75 {
        // A footpath: packed earth worn bare down the middle, with grass and weeds creeping in from
        // both sides.
        col = surface * vec3<f32>(0.78, 0.68, 0.52) * 0.85 * (0.85 + 0.3 * patches);
        let worn = 1.0 - smoothstep(0.1, hw, lane);
        col = mix(col * 0.82, col * 1.05, worn * 0.5);
        let green = vec3<f32>(0.20, 0.28, 0.10) * (0.7 + 0.6 * grain);
        let verge = smoothstep(hw * 0.35, hw, lane) * smoothstep(0.25, 0.65, grain + 0.3 * edge);
        col = mix(col, green, verge * 0.85);
        roughness = 1.0;
    } else if track < 0.5 {
        // Tarmac: chip-seal texture darkened to a worn road, with patched repairs, darker wheel
        // paths, and a crumbling edge where it meets the verge.
        col = surface * vec3<f32>(0.50, 0.50, 0.52) * (0.86 + 0.28 * patches);
        col *= 1.0 - 0.12 * wheels * smoothstep(0.25, 0.7, fbm2(wp / 3.5));
        let crumble = smoothstep(0.35, 0.8, grain + 0.35 * edge);
        col = mix(col, vec3<f32>(0.15, 0.125, 0.09) * (0.7 + 0.6 * grain), edge * crumble * 0.85);
        // Paint, worn and flaking in places.
        var paint = max(centre_line, edge_line * edge_painted) * painted;
        paint *= 0.55 + 0.45 * smoothstep(0.18, 0.55, fbm2(vec2<f32>(across * 5.0, along * 2.5) + 31.7));
        col = mix(col, vec3<f32>(0.80, 0.80, 0.76), paint * 0.92);
        roughness = mix(0.92, 0.62, paint);
    } else {
        // A farm track: gravel, with two worn ruts and a strip of grass between them, and grass
        // creeping in from the verges.
        col = surface * vec3<f32>(0.86, 0.80, 0.70) * 0.8 * (0.85 + 0.3 * patches);
        col *= 1.0 - 0.28 * ruts;
        let strip = (1.0 - smoothstep(0.25, 0.7, lane)) * smoothstep(0.35, 0.65, fbm2(wp / 0.6));
        let green = vec3<f32>(0.20, 0.28, 0.10) * (0.7 + 0.6 * grain);
        col = mix(col, green, strip * 0.8);
        let verge = smoothstep(hw - 0.9, hw, lane) * smoothstep(0.3, 0.7, grain + 0.2 * edge);
        col = mix(col, green * 1.1, verge * 0.8);
        roughness = 1.0;
    }

    pbr_input.material.base_color = vec4<f32>(col, 1.0);
    pbr_input.material.perceptual_roughness = roughness;
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
