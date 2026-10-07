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

// The slope of the water surface at a point: six travelling waves from different directions
// and of different lengths, each with the speed deep-water physics gives it. Unlike a noise
// field there is no lattice to show through as straight lines, and the slopes are exact. The
// short waves fade out with distance, where they would be sub-pixel and sparkle as noise.
fn wave_slope(p: vec2<f32>, t: f32, dist: f32) -> vec2<f32> {
    // (direction x, direction z, wavelength in metres, slope amplitude)
    var g = vec2<f32>(0.0);
    g += wave(p, t, vec2<f32>(0.97, 0.26), 9.0, 0.17);
    g += wave(p, t, vec2<f32>(-0.40, 0.92), 5.3, 0.14);
    g += wave(p, t, vec2<f32>(0.60, -0.80), 3.1, 0.12) * (1.0 - smoothstep(120.0, 450.0, dist));
    g += wave(p, t, vec2<f32>(-0.90, -0.43), 1.9, 0.10) * (1.0 - smoothstep(55.0, 190.0, dist));
    g += wave(p, t, vec2<f32>(0.20, 0.98), 1.1, 0.08) * (1.0 - smoothstep(25.0, 95.0, dist));
    g += wave(p, t, vec2<f32>(-0.75, 0.66), 0.65, 0.06) * (1.0 - smoothstep(12.0, 45.0, dist));
    return g;
}

fn wave(p: vec2<f32>, t: f32, dir: vec2<f32>, wavelength: f32, slope: f32) -> vec2<f32> {
    let d = normalize(dir);
    let k = 6.2831853 / wavelength;
    let omega = sqrt(9.81 * k);
    return d * slope * cos(k * dot(d, p) - omega * t * 0.6);
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
    let steep = mix(0.5, 1.5, smoothstep(0.0, 4.0, depth)) * mix(1.0, 0.15, smoothstep(80.0, 2500.0, dist));
    let g = wave_slope(p, t, dist);
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
