#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
}

// xyz: toward the sun; w: sun illuminance (lux)
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> sun: vec4<f32>;
// x: cloud base altitude, y: top altitude, z: coverage, w: density
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> layer: vec4<f32>;

const PI: f32 = 3.14159265;
const WIND: vec3<f32> = vec3<f32>(5.5, 0.0, 2.0);   // metres per second the cloud field drifts
const MAX_DISTANCE: f32 = 32000.0;
const STEPS: i32 = 20;
const LIGHT_STEPS: i32 = 2;

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

fn fbm3(p: vec3<f32>) -> f32 {
    return 0.55 * noise3(p) + 0.27 * noise3(p * 2.03 + 11.7) + 0.13 * noise3(p * 4.1 + 3.9) + 0.05 * noise3(p * 8.3);
}

// Cheaper fractal noise for the coarse layers, which don't need the finest octaves.
fn fbm3_lo(p: vec3<f32>) -> f32 {
    return 0.6 * noise3(p) + 0.28 * noise3(p * 2.03 + 11.7) + 0.12 * noise3(p * 4.1 + 3.9);
}

fn hg(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI * pow(1.0 + g2 - 2.0 * g * cos_theta, 1.5));
}

// How dense the cloud is at a point (0 = clear sky).
fn density(world: vec3<f32>, detail: bool) -> f32 {
    let t = globals.time;
    let p = world + WIND * t;
    let base = layer.x;
    let top = layer.y;
    let h = clamp((world.y - base) / (top - base), 0.0, 1.0);
    // A flat-ish base and a rounded, billowing top.
    let profile = smoothstep(0.0, 0.1, h) * (1.0 - smoothstep(0.5, 1.0, h));
    // Large-scale coverage decides where there are clouds at all; the shape noise sculpts them.
    let cover = fbm3_lo(vec3<f32>(p.x, p.y * 0.2, p.z) / 9500.0);
    let coarse = (cover - (1.0 - layer.z) * 0.9) * 4.5;
    // The shape noise adds at most 0.9, so where coverage is this low there is certainly clear
    // sky: skip the expensive noise (this is most of the sky, most of the time).
    if profile <= 0.0 || coarse + 0.9 <= 0.0 {
        return 0.0;
    }
    let shape = fbm3_lo(p / 1500.0 + vec3<f32>(0.0, t * 0.004, 0.0));
    var d = coarse + (shape - 0.5) * 1.8;
    if detail && d > -0.35 {
        // Erode the edges with finer noise, which is what makes cauliflower billows.
        d -= (fbm3_lo(p / 330.0) - 0.35) * 0.7;
    }
    let dc = clamp(d, 0.0, 1.0);
    // An S-curve thins the fringes and thickens the cores, so edges read crisp.
    return dc * dc * (3.0 - 2.0 * dc) * profile * layer.w;
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

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
    out.color = vec4<f32>(0.0);

    let origin = view.world_position;
    let dir = normalize(in.world_position.xyz - origin);
    let range = slab(origin, dir);
    if range.y <= range.x {
        return out;
    }

    // Jitter the start of the ray per pixel, so the limited number of steps shows as fine
    // noise rather than visible bands.
    let jitter = fract(52.9829189 * fract(dot(in.position.xy, vec2<f32>(0.06711056, 0.00583715))));
    let step_len = (range.y - range.x) / f32(STEPS);
    var t = range.x + step_len * jitter;

    let to_sun = normalize(sun.xyz);
    let cos_sun = dot(dir, to_sun);
    // Forward-scattering silver lining plus a little back-scatter, as real cloud droplets do.
    let phase = mix(hg(cos_sun, 0.62), hg(cos_sun, -0.25), 0.3);
    let sun_light = vec3<f32>(1.0, 0.96, 0.90) * sun.w * 3.2;
    let sky_light = vec3<f32>(0.55, 0.66, 0.88) * sun.w * 0.11;

    var transmittance = 1.0;
    var colour = vec3<f32>(0.0);
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
            let scattered = (sun_light * phase * sun_transmit * mix(0.45, 1.0, powder) + ambient) * (1.0 / PI) * 4.0 * PI * 0.25;
            let extinction = d * 0.0075;
            let dt = step_len;
            let step_t = exp(-extinction * dt);
            colour += transmittance * (1.0 - step_t) * scattered;
            transmittance *= step_t;
        }
        t += step_len;
    }
    var alpha = 1.0 - transmittance;

    // Distant cloud melts into the haze of the horizon, and the dome's own edge fades out.
    let haze = smoothstep(9000.0, 30000.0, range.x);
    let horizon = smoothstep(0.015, 0.09, dir.y);
    alpha *= (1.0 - haze * 0.85) * horizon;
    colour = mix(colour, vec3<f32>(0.62, 0.70, 0.82) * sun.w * 0.05, haze * 0.7) * horizon;

    // Everything else in the scene is multiplied by the camera's exposure; so must this be.
    out.color = vec4<f32>(colour * view.exposure, alpha);
    return out;
}
