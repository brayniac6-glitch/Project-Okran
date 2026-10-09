// Kenshi's fog volumes, ported from data/materials/post/fog.hlsl (fog_sphere_fs, fog_beam_fs,
// fogValue): the length of the view ray inside the sphere or the beam, cut by the scene's depth,
// as fog. Drawn on the hull's far side with no depth test (fog_volume.rs).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view
#import bevy_pbr::view_transformations::{depth_ndc_to_view_z, position_world_to_view}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif

struct FogVolumeParams {
    // xyz: centre or start (scene space); w: radius
    start: vec4<f32>,
    // xyz: the beam's axis; w: its length (0: a sphere)
    axis: vec4<f32>,
    colour: vec4<f32>,
    // x: density; y: edgeBlur; z: additive
    consts: vec4<f32>,
    // xyz: the scene origin in world units
    origin: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: FogVolumeParams;

// fogValue: the amount through an S curve
fn fog_value(depth: f32, density: f32, colour: vec4<f32>) -> vec4<f32> {
    let amount = clamp(depth * density, 0.0, 1.0);
    var curve = 2.0 * amount * amount;
    if (amount >= 0.5) {
        curve = -2.0 * (amount - 1.0) * (amount - 1.0) + 1.0;
    }
    return vec4<f32>(colour.rgb, curve * colour.a);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let camera = view.world_position.xyz;
    let to_frag = in.world_position.xyz - camera;
    let ray = normalize(to_frag);
    // the scene's distance along the ray (none: the far clip)
    var distance = 1.0e7;
#ifdef DEPTH_PREPASS
    let ndc = prepass_depth(in.position, 0u);
    if (ndc > 0.0) {
        let scene_z = depth_ndc_to_view_z(ndc);
        let frag_z = position_world_to_view(in.world_position.xyz).z;
        distance = length(to_frag) * (scene_z / frag_z);
    }
#endif
    var t0 = 0.0;
    var t1 = 0.0;
    var density = p.consts.x;
    let m = camera - p.start.xyz;
    if (p.axis.w <= 0.0) {
        // sphere intersections
        let b = dot(m, ray);
        let c = dot(m, m) - p.start.w * p.start.w;
        let root = sqrt(max(0.0, b * b - c));
        t0 = clamp(-b - root, 0.0, distance);
        t1 = clamp(-b + root, 0.0, distance);
        // break up the sphere in the distance
        let world = in.world_position.xyz + p.origin.xyz;
        var ss = sin(world.y * 0.004) + 1.0;
        ss = ss - (cos(world.z * 0.0008) * cos(world.x * 0.0008) - 1.0);
        ss = ss * clamp(t0 * 0.0001 - 1.0, 0.0, 1.0) * 1000.0;
        t1 = t1 - ss;
    } else {
        // cylinder intersections
        let axis = p.axis.xyz;
        let md = dot(m, axis);
        let nd = dot(ray, axis);
        let mn = dot(m, ray);
        let a = max(1.0 - nd * nd, 1.0e-6);
        let k = dot(m, m) - p.start.w * p.start.w;
        let c = k - md * md;
        let b = mn - nd * md;
        let root = sqrt(max(0.0, b * b - a * c));
        t0 = clamp((-b - root) / a, 0.0, distance);
        t1 = clamp((-b + root) / a, 0.0, distance);
        // the ends (the start relative to the camera, as fog.hlsl's view space has it)
        let s = -m;
        let e0 = dot(axis, s) / nd;
        let e1 = dot(axis, s + axis * p.axis.w) / nd;
        t0 = max(t0, select(e1, e0, nd > 0.0));
        t1 = min(t1, select(e1, e0, nd < 0.0));
        // end fade
        let ctr = ray * (t0 + t1) * 0.5;
        let d = dot(ctr, axis);
        var edge = clamp((d - e0 * nd) * p.consts.y, 0.0, 1.0);
        edge = edge * clamp((-d + e1 * nd) * p.consts.y, 0.0, 1.0);
        edge = 1.0 - pow(1.0 - edge, 2.0);
        density = density * edge;
    }
    return fog_value(max(t1 - t0, 0.0), density, p.colour);
}
