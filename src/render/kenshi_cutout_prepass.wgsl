// The depth (and shadow) prepass for Kenshi's cut-out materials: the same alpha cuts as their
// main passes (objects.hlsl TRANSPARENCY, skin.hlsl's clothing cut, hair_fs), so cut-out pixels
// leave no depth behind. CHARACTER picks the character material's layout, else the object's.

#import bevy_pbr::prepass_io::VertexOutput
#import kenshi::interior::interior_clipped
#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput
#endif

struct Params {
    // character: (kind, glossMult, threshold, water height); object: (tiling, glossMult, threshold)
    first: vec4<f32>,
#ifdef CHARACTER
    rest: array<vec4<f32>, 10>,
    hair_mult: vec4<f32>,
    hair_alpha: vec4<f32>,
    beard_alpha: vec4<f32>,
#else
    // the object's other parameters, then CONSTRUCTION (ObjectParams)
    rest: array<vec4<f32>, 14>,
    construction: vec4<f32>,
#endif
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: Params;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var normal_map: texture_2d<f32>;
#ifndef CHARACTER
@group(#{MATERIAL_BIND_GROUP}) @binding(16) var grid_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(17) var interior_mask: texture_2d<f32>;
#endif

fn cut(in: VertexOutput) {
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2<f32>(0.0);
#endif
#ifdef CHARACTER
    let kind = i32(p.first.x);
    if (kind == 1) {
        if (textureSample(normal_map, diffuse_sampler, uv).a - 0.6 < 0.0) {
            discard;
        }
    } else if (kind == 2) {
        let a = textureSampleBias(diffuse_map, diffuse_sampler, uv, -2.0) * p.hair_alpha;
        if (a.r + a.g + a.b + a.a - p.beard_alpha.x < 0.0) {
            discard;
        }
    }
#else
    // CLIP_INTERIOR on grass and foliage (ObjectParams world.y), as the main pass
    if (p.rest[6].y > 0.5 && interior_clipped(interior_mask, in.position, in.world_position.xyz)) {
        discard;
    }
    // the main pass's coverage boost (kenshi_object.wgsl, an enhancement)
    let size = vec2<f32>(textureDimensions(normal_map));
    let dx = dpdx(uv * p.first.xy * size);
    let dy = dpdy(uv * p.first.xy * size);
    let coverage = min(1.0 + max(0.0, 0.5 * log2(max(dot(dx, dx), dot(dy, dy)))) * 0.25, 1.6);
    if (p.first.w > 0.0) {
        if (textureSampleBias(normal_map, diffuse_sampler, uv * p.first.xy, -0.5).a * coverage < p.first.w) {
            discard;
        }
    }
    // CONSTRUCTION: only the scaffold's bars above 1.2 x the progress (construction.hlsl's
    // shadow caster)
    let grid = textureSample(grid_map, diffuse_sampler, uv * p.construction.z).a;
    if (p.construction.y > 0.0) {
        let height = (in.world_position.y - p.construction.w) / p.construction.y;
        if (p.construction.x * 1.2 < height && grid - 0.7 < 0.0) {
            discard;
        }
    }
#endif
}

#ifdef PREPASS_FRAGMENT
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    cut(in);
    var out: FragmentOutput;
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
    return out;
}
#else
@fragment
fn fragment(in: VertexOutput) {
    cut(in);
}
#endif
