#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::globals,
}

// x: sway at full height (m), y: height at which full sway is reached, z: leaf flutter (m)
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> wind: vec4<f32>;
// xy: the direction the world's wind blows along, z: its strength relative to the tuned breeze
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> world_wind: vec4<f32>;

@vertex
fn vertex(vertex_no_morph: Vertex) -> VertexOutput {
    var out: VertexOutput;
    var vertex = vertex_no_morph;
    let world_from_local = mesh_functions::get_world_from_local(vertex_no_morph.instance_index);

#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex_no_morph.instance_index);
#endif

#ifdef VERTEX_POSITIONS
    var world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));

    // Each object sways in its own phase, taken from where it stands, inside a slow gust
    // field that rolls across the map so neighbouring trees move together.
    let origin = world_from_local[3].xyz;
    let t = globals.time;
    let phase = origin.x * 0.071 + origin.z * 0.053;
    let gust = 0.55 + 0.45 * sin(t * 0.4 + origin.x * 0.0045 + origin.z * 0.0031);
    // Bend grows faster than linearly with height, so the base stays planted.
    let h = clamp(vertex.position.y / max(wind.y, 0.01), 0.0, 1.4);
    let bend = pow(h, 1.5) * wind.x * gust * world_wind.z;
    // Mostly a lean downwind, plus a smaller back-and-forth along and across the wind.
    let along = world_wind.xy;
    let across = vec2<f32>(-along.y, along.x);
    let lean = 0.75 + 0.45 * sin(t * 1.25 + phase);
    let side = 0.35 * cos(t * 0.95 + phase * 1.3);
    let sway = along * lean + across * side;
    world_position.x += sway.x * bend;
    world_position.z += sway.y * bend;
    // Quick, small flutter of individual leaves, out of phase from vertex to vertex.
    let flutter = sin(t * 5.5 + dot(vertex.position, vec3<f32>(1.7, 2.3, 1.1)) + phase) * wind.z * smoothstep(0.2, 0.8, h) * gust * world_wind.z;
    world_position.x += flutter;
    world_position.y += flutter * 0.5;

    out.world_position = world_position;
    out.position = position_world_to_clip(world_position.xyz);
#endif

#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex_no_morph.instance_index);
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex_no_morph.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex_no_morph.instance_index, world_from_local[3]);
#endif
    return out;
}
