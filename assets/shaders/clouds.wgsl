#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
}

// xyz: toward the sun; w: sun illuminance (lux)
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> sun: vec4<f32>;
// x: cloud base altitude, y: top altitude, z: coverage, w: density
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> layer: vec4<f32>;
// xy: how far the whole cloud field has drifted on the wind (metres), z: cirrus cover
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<uniform> drift: vec4<f32>;

const PI: f32 = 3.14159265;
const MAX_DISTANCE: f32 = 32000.0;
const STEPS: i32 = 16;
const LIGHT_STEPS: i32 = 2;
const CIRRUS_ALTITUDE: f32 = 9200.0;

fn hash31(p: vec3<f32>) -> f32 {
    var q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yxz + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(mix(hash31(i), hash31(i + vec3<f32>(1.0, 0.0, 0.0)), u.x),
            mix(hash31(i + vec3<f32>(0.0, 1.0, 0.0)), hash31(i + vec3<f32>(1.0, 1.0, 0.0)), u.x), u.y),
        mix(mix(hash31(i + vec3<f32>(0.0, 0.0, 1.0)), hash31(i + vec3<f32>(1.0, 0.0, 1.0)), u.x),
            mix(hash31(i + vec3<f32>(0.0, 1.0, 1.0)), hash31(i + vec3<f32>(1.0, 1.0, 1.0)), u.x), u.y),
        u.z,
    );
}

// Fractal noise for the cloud layer. The same function, constant for constant, is in
// cloud_shadows.rs, which bakes the shadows the clouds cast; keep the two in step.
fn fbm3_lo(p: vec3<f32>) -> f32 {
    return 0.6 * noise3(p) + 0.28 * noise3(p * 2.03 + 11.7) + 0.12 * noise3(p * 4.1 + 3.9);
}

fn hg(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI * pow(1.0 + g2 - 2.0 * g * cos_theta, 1.5));
}

// How dense the cloud is at a point (0 = clear sky). Two kinds of cloud mix across the sky in
// large regions: puffy, tall-topped cumulus, and flat, layered stratocumulus sheets.
fn density(world: vec3<f32>, detail: bool) -> f32 {
    // The field drifts with the wind: what is at `world` now was at `world - drift` before.
    let p = world - vec3<f32>(drift.x, 0.0, drift.y);
    let base = layer.x;
    let top = layer.y;
    let h = clamp((world.y - base) / (top - base), 0.0, 1.0);

    // Regional type: 0 is puffy cumulus, 1 is flat stratocumulus.
    let typ = fbm3_lo(vec3<f32>(p.x, 0.0, p.z) / 26000.0 + vec3<f32>(5.2, 0.0, 1.3));
    let flat_k = smoothstep(0.53, 0.68, typ);
    let profile_cumulus = smoothstep(0.0, 0.1, h) * (1.0 - smoothstep(0.5, 1.0, h));
    let profile_strato = smoothstep(0.0, 0.14, h) * (1.0 - smoothstep(0.3, 0.5, h));
    let profile = mix(profile_cumulus, profile_strato, flat_k);

    // Large-scale coverage decides where there are clouds at all; stratocumulus forms
    // more continuous sheets, so it needs less coverage to spread.
    let cover = fbm3_lo(vec3<f32>(p.x, p.y * 0.2, p.z) / 9500.0);
    let coarse = (cover - (1.0 - layer.z - flat_k * 0.03) * 0.9) * 4.5;
    // The shape noise adds at most 0.9, so where coverage is this low there is certainly clear
    // sky: skip the expensive noise (this is most of the sky, most of the time).
    if profile <= 0.0 || coarse + 0.9 <= 0.0 {
        return 0.0;
    }
    // Stratocumulus features are wide and shallow; cumulus ones are tall.
    let squash = mix(1.0, 2.6, flat_k);
    let shape = fbm3_lo(vec3<f32>(p.x, p.y * squash, p.z) / mix(1500.0, 650.0, flat_k));
    // Stratocumulus is cellular: strong, small-scale lumps with clear gaps between them.
    var d = coarse + (shape - 0.5) * mix(1.8, 2.7, flat_k);
    if detail && d > -0.35 {
        // Erode the edges with finer noise, which is what makes cauliflower billows.
        d -= (fbm3_lo(p / mix(330.0, 200.0, flat_k)) - 0.35) * mix(0.7, 0.8, flat_k);
    }
    let dc = clamp(d, 0.0, 1.0);
    // An S-curve thins the fringes and thickens the cores, so edges read crisp.
    return dc * dc * (3.0 - 2.0 * dc) * profile * layer.w * mix(1.0, 0.45, flat_k);
}

// Where a ray leaves the slab of sky the clouds live in; x = entry distance, y = exit distance.
fn slab(origin: vec3<f32>, dir: vec3<f32>) -> vec2<f32> {
    if dir.y < 0.015 {
        return vec2<f32>(1.0, 0.0);   // looking at or below the horizon: no clouds
    }
    let t0 = max((layer.x - origin.y) / dir.y, 0.0);
    let t1 = (layer.y - origin.y) / dir.y;
    return vec2<f32>(t0, min(t1, MAX_DISTANCE));
}

fn fbm2(p: vec2<f32>) -> f32 {
    return fbm3_lo(vec3<f32>(p.x, 0.0, p.y));
}

// High, thin cirrus: wisps drawn out into streaks along the wind, on a single plane very far
// above the weather. It's thin enough to need no ray-marching.
fn cirrus(origin: vec3<f32>, dir: vec3<f32>, to_sun: vec3<f32>) -> vec4<f32> {
    if dir.y < 0.03 {
        return vec4<f32>(0.0);
    }
    let t = (CIRRUS_ALTITUDE - origin.y) / dir.y;
    if t > 90000.0 {
        return vec4<f32>(0.0);
    }
    let along = normalize(vec2<f32>(drift.x, drift.y) + vec2<f32>(0.0001, 0.0));
    let across = vec2<f32>(-along.y, along.x);
    // High winds move the cirrus faster than the clouds below.
    let hit = origin.xz + dir.xz * t - vec2<f32>(drift.x, drift.y) * 1.7;
    let u = dot(hit, along);
    let v = dot(hit, across);
    // Streaks: stretched about 7:1 along the wind, with a gentle bend from a second noise.
    let warp = (fbm2(hit / 14000.0) - 0.5) * 6000.0;
    let n = fbm2(vec2<f32>(u / 24000.0, (v + warp) / 3400.0) + vec2<f32>(3.1, 8.7));
    let fine = fbm2(vec2<f32>(u / 7000.0, v / 900.0) + vec2<f32>(1.3, 4.1));
    let thin = smoothstep(0.62 - drift.z * 0.2, 0.92, n * 0.8 + fine * 0.35);
    let horizon = smoothstep(0.03, 0.14, dir.y);
    let alpha = thin * 0.55 * horizon;
    // Lit by the sun from above, faintly pink-gold near the sun's side.
    let facing = max(dot(dir, to_sun), 0.0);
    let col = vec3<f32>(1.0, 0.97, 0.93) * sun.w * (0.22 + 0.4 * pow(facing, 6.0)) + vec3<f32>(0.5, 0.6, 0.85) * sun.w * 0.05;
    return vec4<f32>(col * alpha, alpha);
}

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
    out.color = vec4<f32>(0.0);

    let origin = view.world_position;
    let dir = normalize(in.world_position.xyz - origin);
    let to_sun = normalize(sun.xyz);

    // The wispy high layer first (it's behind everything else), as premultiplied colour+alpha.
    let high = cirrus(origin, dir, to_sun);

    var colour = vec3<f32>(0.0);
    var alpha = 0.0;
    let range = slab(origin, dir);
    if range.y > range.x {
        // Jitter the start of the ray per pixel, so the limited number of steps shows as fine
        // noise rather than visible bands.
        let jitter = fract(52.9829189 * fract(dot(in.position.xy, vec2<f32>(0.06711056, 0.00583715))));
        let step_len = (range.y - range.x) / f32(STEPS);
        var t = range.x + step_len * jitter;

        let cos_sun = dot(dir, to_sun);
        // Forward-scattering silver lining plus a little back-scatter, as real cloud droplets do.
        let phase = mix(hg(cos_sun, 0.62), hg(cos_sun, -0.25), 0.3);
        let sun_light = vec3<f32>(1.0, 0.96, 0.90) * sun.w * 3.2;
        let sky_light = vec3<f32>(0.55, 0.66, 0.88) * sun.w * 0.11;

        var transmittance = 1.0;
        for (var i = 0; i < STEPS; i++) {
            if transmittance < 0.02 {
                break;
            }
            let pos = origin + dir * t;
            let d = density(pos, true);
            if d > 0.002 {
                // Darkness toward the sun: march a few steps that way, summing density.
                var along = 0.0;
                for (var j = 1; j <= LIGHT_STEPS; j++) {
                    let lp = pos + to_sun * (f32(j) * f32(j) * 170.0);
                    along += density(lp, false) * f32(j) * 170.0;
                }
                let sun_transmit = exp(-along * 0.0016);
                // "Powder": thin edges scatter less than thick cores, which darkens cloud edges.
                let powder = 1.0 - exp(-d * 5.0);
                let h = clamp((pos.y - layer.x) / (layer.y - layer.x), 0.0, 1.0);
                let ambient = sky_light * mix(0.55, 1.0, h);
                let scattered = sun_light * phase * sun_transmit * mix(0.45, 1.0, powder) + ambient;
                let extinction = d * 0.0075;
                let step_t = exp(-extinction * step_len);
                colour += transmittance * (1.0 - step_t) * scattered;
                transmittance *= step_t;
            }
            t += step_len;
        }
        alpha = 1.0 - transmittance;

        // Distant cloud melts into the haze of the horizon.
        let haze = smoothstep(9000.0, 30000.0, range.x);
        alpha *= 1.0 - haze * 0.85;
        colour = mix(colour, vec3<f32>(0.62, 0.70, 0.82) * sun.w * 0.05, haze * 0.7);
    }

    // The dome's own lower edge fades out into the horizon.
    let horizon = smoothstep(0.015, 0.09, dir.y);
    alpha *= horizon;
    colour *= horizon;

    // Cumulus in front of the cirrus: the cirrus shows through wherever the cloud is thin.
    let rgb = colour + high.rgb * (1.0 - alpha);
    let a = alpha + high.a * (1.0 - alpha);

    // Everything else in the scene is multiplied by the camera's exposure; so must this be.
    out.color = vec4<f32>(rgb * view.exposure, a);
    return out;
}
