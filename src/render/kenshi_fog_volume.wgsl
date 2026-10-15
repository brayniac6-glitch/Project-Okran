// Kenshi's fog volumes, ported from data/materials/post/fog.hlsl (fog_sphere_fs, fog_beam_fs,
// fogValue): the length of the view ray inside the sphere or the beam, cut by the scene's depth,
// as fog. Drawn on the hull's far side with no depth test (fog_volume.rs).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view
#import bevy_pbr::view_transformations::{depth_ndc_to_view_z, position_world_to_view}
#import kenshi::lighting::atmosphere_fog
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
    // a plane volume's planes (world: normal, w; inside where normal . p < w)
    planes: array<vec4<f32>, 7>,
    // sunDirectionReal, sunColour.w; the atmosphere's horizon and fog parameters; the global
    // fog's colour
    sun_real: vec4<f32>,
    horizon: vec4<f32>,
    fog: vec4<f32>,
    fog_colour: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: FogVolumeParams;

// fog_planes_fs: the ray (from the world camera) between the volume's planes, cut by the scene
fn fog_planes(camera: vec3<f32>, ray: vec3<f32>, distance: f32) -> vec4<f32> {
    var far = distance;
    var near = 0.0;
    for (var i = 0; i < 7; i = i + 1) {
        let pl = p.planes[i];
        let dn = dot(pl.xyz, ray);
        let d = (pl.w - dot(pl.xyz, camera)) / dn;
        if (dn < 0.0) {
            near = max(near, d);
        } else {
            far = min(far, d);
        }
    }
    near = min(near, distance);
    // fog super far distance fade out
    let edge_blur = p.consts.y * clamp(1.0 / (far * 0.00006), 0.0, 1.0);
    // edge fixes
    var edge = 1.0;
    let ctr = camera + ray * (far + near) * 0.5;
    for (var i = 0; i < 7; i = i + 1) {
        let pl = p.planes[i];
        edge = edge * clamp((pl.w - dot(pl.xyz, ctr)) * edge_blur, 0.0, 1.0);
    }
    edge = edge * (1.0 + abs(ray.y) * 0.9);
    let amount = far - near;
    let brighten = 1.6 * clamp(p.sun_real.y * 3.0 + 0.2, 0.0, 1.0);
    let fog = lit_fog_value(amount, p.consts.x * edge, p.colour, max(0.08, p.sun_real.w * brighten));
    if (near == 0.0) {
        return fog;
    }
    // fog the fog: the atmosphere and the global fog in front of it
    let world_pos = camera + ray * near;
    var atmosphere = atmosphere_fog(camera, world_pos, near, p.sun_real.xyz, p.horizon, p.fog, p.fog.x * 0.95);
    let global = lit_fog_value(near, p.fog.w, p.fog_colour, p.sun_real.w);
    atmosphere.a = atmosphere.a * fog.a;
    var r = mix(fog, clamp(atmosphere, vec4<f32>(0.0), vec4<f32>(4.0)), clamp((near - 12000.0) * 0.0001, 0.0, 1.0));
    r = mix(r, global, global.a);
    r.a = clamp(r.a + global.a, 0.0, 1.0) * fog.a;
    return r;
}

// fogValue with its light (the colour times it)
fn lit_fog_value(depth: f32, density: f32, colour: vec4<f32>, light: f32) -> vec4<f32> {
    let f = fog_value(depth, density, colour);
    return vec4<f32>(f.rgb * light, f.a);
}

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
    if (p.consts.w > 0.5) {
        return fog_planes(camera + p.origin.xyz, ray, distance);
    }
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
