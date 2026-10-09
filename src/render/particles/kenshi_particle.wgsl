// Kenshi's particle shaders (data/materials/forward/basic.hlsl basic_vp / basic_fp; the vertex
// colour (COLOURED), dimmed by the sun's height (AMBIENT); particles.hlsl depth_blend_fp for
// Particle_Blend_Depth). The output is the HDR scene's (no fog: particles are drawn after the
// atmosphere fog pass). Not ported: CLIP_INTERIOR (the interior mask).

#import bevy_pbr::mesh_functions::get_world_from_local
#import bevy_pbr::mesh_view_bindings::view
#import bevy_pbr::view_transformations::depth_ndc_to_view_z
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif

struct Params {
    // rgb: the AMBIENT multiplier (1 without)
    colour_mult: vec4<f32>,
    // x: COLOURED; y: depth blend; z: the far clip
    flags: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: Params;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var samp: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) colour: vec4<f32>,
}

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) view_pos: vec3<f32>,
}

@vertex
fn vertex(v: Vertex) -> VOut {
    var out: VOut;
    let world = get_world_from_local(v.instance_index) * vec4<f32>(v.position, 1.0);
    out.clip = view.clip_from_world * world;
    out.uv = v.uv;
    var c = vec4<f32>(1.0);
    if (p.flags.x > 0.5) {
        c = v.colour;
    }
    out.colour = vec4<f32>(c.rgb * p.colour_mult.rgb, c.a);
    out.view_pos = (view.view_from_world * world).xyz;
    return out;
}

@fragment
fn fragment(in: VOut) -> @location(0) vec4<f32> {
    var fragment = textureSample(tex, samp, in.uv) * in.colour;
    if (p.flags.y > 0.5) {
        // depth_blend_fp: the distance behind (the G-buffer's, the far clip where nothing is),
        // less 100; alpha faded in over the first 10 units and toward what is behind
        let far = p.flags.z;
        var distance = far;
#ifdef DEPTH_PREPASS
        let ndc = prepass_depth(in.clip, 0u);
        if (ndc > 0.0) {
            let behind_z = -depth_ndc_to_view_z(ndc);
            let ld_z = -in.view_pos.z;
            distance = length(in.view_pos) * behind_z / max(ld_z, 1e-4);
        }
#endif
        distance = distance - 100.0;
        let ld = -in.view_pos.z;
        let alpha = clamp(ld * 0.1, 0.0, 1.0) * clamp((distance - ld) * 0.01, 0.0, 1.0);
        fragment.a = fragment.a * alpha;
    }
    return fragment;
}
