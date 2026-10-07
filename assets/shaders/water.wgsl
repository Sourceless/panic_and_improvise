#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{globals, view},
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var bed_heights: texture_2d<f32>;
// x: half the map size, y: cell size, z: vertices per side
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> map_info: vec4<f32>;

// The terrain's height under a point, bilinearly interpolated from the height texture. Outside
// the map it clamps to the edge, which (since the edge is sea floor) is deep water.
fn bed_height(p: vec2<f32>) -> f32 {
    let n = i32(map_info.z);
    let g = (p + vec2<f32>(map_info.x)) / map_info.y;
    let i = floor(g);
    // Eased interpolation: the slope is zero at every cell edge, so the depth (and everything
    // derived from it) has no visible creases along the 10 m grid, which plain bilinear has.
    let t = g - i;
    let f = t * t * (3.0 - 2.0 * t);
    let x0 = clamp(i32(i.x), 0, n - 1);
    let z0 = clamp(i32(i.y), 0, n - 1);
    let x1 = clamp(x0 + 1, 0, n - 1);
    let z1 = clamp(z0 + 1, 0, n - 1);
    let h00 = textureLoad(bed_heights, vec2<i32>(x0, z0), 0).r;
    let h10 = textureLoad(bed_heights, vec2<i32>(x1, z0), 0).r;
    let h01 = textureLoad(bed_heights, vec2<i32>(x0, z1), 0).r;
    let h11 = textureLoad(bed_heights, vec2<i32>(x1, z1), 0).r;
    return mix(mix(h00, h10, f.x), mix(h01, h11, f.x), f.y);
}

// x: how far to turn the waves from their authored direction (radians), y: roughness
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<uniform> sea: vec4<f32>;

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

// The slope of the water surface at a point: a sum of ten travelling waves.
//
// A handful of sine waves of similar size line their crests up into a visible lattice, so these
// are spread over a wide range of wavelengths (each about 1.45x the last, an irrational-ish
// ratio), fan out around the wind direction, swell and fade slowly along their crests, and the
// whole field is gently warped, so no regular pattern can form. Each wave has the speed deep-
// water physics gives it. Unlike a noise field there is no lattice to show through either.
//
// `pixel` is how many metres of water one screen pixel covers here, which is huge at glancing
// angles and near the horizon. A wave shorter than a few pixels can't be resolved and would
// alias into moire stripes, so each wave fades out as its wavelength approaches the pixel size.
fn wave_slope(p_in: vec2<f32>, t: f32, pixel: f32) -> vec2<f32> {
    // (direction angle in radians, wavelength in metres, slope amplitude)
    var waves = array<vec3<f32>, 10>(
        vec3<f32>(0.30, 14.0, 0.15),
        vec3<f32>(0.85, 9.7, 0.14),
        vec3<f32>(-0.32, 6.7, 0.13),
        vec3<f32>(1.45, 4.6, 0.11),
        vec3<f32>(-0.75, 3.2, 0.10),
        vec3<f32>(0.55, 2.2, 0.085),
        vec3<f32>(-0.05, 1.5, 0.07),
        vec3<f32>(1.15, 1.05, 0.06),
        vec3<f32>(-0.50, 0.72, 0.05),
        vec3<f32>(0.40, 0.50, 0.04),
    );
    // Warp the sampling position by a slow, large-scale swirl.
    let p = p_in + 1.6 * vec2<f32>(sin(0.11 * p_in.y + 0.31 * t), sin(0.09 * p_in.x - 0.27 * t));
    var g = vec2<f32>(0.0);
    for (var i = 0; i < 10; i++) {
        let w = waves[i];
        // Turned with the wind: the whole wave field rotates to run along it.
        let d = vec2<f32>(cos(w.x + sea.x), sin(w.x + sea.x));
        let k = 6.2831853 / w.y;
        let omega = sqrt(9.81 * k);
        // A wave needs several pixels per wavelength to be sampled cleanly, or it aliases:
        // full strength at 16+ pixels per wavelength, gone entirely by 5.
        let resolved = smoothstep(0.0625, 0.2, pixel / w.y);
        // Swells and fades slowly along its own crest.
        let across = dot(vec2<f32>(-d.y, d.x), p);
        let swell = 0.65 + 0.35 * sin(across * 0.05 * (1.0 + 0.3 * f32(i)) + f32(i) * 1.7 + t * 0.07);
        g += d * w.z * swell * (1.0 - resolved) * cos(k * dot(d, p) - omega * t * 0.6 + f32(i) * 2.1);
    }
    return g;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let p = in.world_position.xz;
    // How deep the water is at this exact pixel: the surface height less the bed beneath it.
    let depth = max(in.world_position.y - bed_height(p), 0.0);
    let t = globals.time;

    // Ripple normal from the height field's slope. Gentler on deep water, where waves are
    // longer, and nearly flat right at the shore.
    let dist = length(view.world_position - in.world_position.xyz);
    // Calmer with distance too, so the horizon reads as a smooth sheet of sky reflection.
    let steep = mix(0.5, 1.5, smoothstep(0.0, 4.0, depth)) * mix(1.0, 0.15, smoothstep(80.0, 2500.0, dist)) * sea.y;
    // World metres per screen pixel, from how fast the position changes across the screen.
    let pixel = max(length(dpdx(p)), length(dpdy(p)));
    let g = wave_slope(p, t, pixel);
    let n = normalize(vec3<f32>(-g.x * steep, 1.0, -g.y * steep));
    pbr_input.N = n;
    pbr_input.world_normal = n;

    // Colour by depth: clear green-blue shallows to dark blue depths.
    let shallow = vec3<f32>(0.07, 0.24, 0.26);
    let deep = vec3<f32>(0.015, 0.07, 0.16);
    var col = mix(shallow, deep, smoothstep(0.1, 7.0, depth));

    // Shoreline foam: a thin, broken line where the water gets very shallow.
    let wobble = vnoise(p * 1.6 + vec2<f32>(t * 0.2, -t * 0.15));
    let foam = (1.0 - smoothstep(0.05, 0.45 + 0.35 * wobble, depth)) * smoothstep(0.25, 0.7, wobble + 0.25);
    col = mix(col, vec3<f32>(0.85, 0.9, 0.92), foam * 0.8);

    // Clear in the shallows (you can see the bed), almost opaque in the deeps, and fading out
    // to nothing at the very edge so there is no hard line where it meets the land.
    var alpha = mix(0.45, 1.0, smoothstep(0.0, 8.0, depth));
    // Seen at a glancing angle water turns into a mirror, so it gets opaque with distance.
    let to_eye = normalize(view.world_position - in.world_position.xyz);
    let fresnel = pow(1.0 - clamp(dot(n, to_eye), 0.0, 1.0), 3.0);
    alpha = max(alpha, fresnel * 1.2);
    alpha = alpha * smoothstep(0.0, 0.12, depth);
    alpha = max(alpha, foam * 0.85);

    pbr_input.material.base_color = vec4<f32>(col, alpha);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
