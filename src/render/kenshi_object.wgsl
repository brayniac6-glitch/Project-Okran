// Kenshi's static objects (buildings): data/materials/deferred/objects.hlsl main_ps and
// triplanar.hlsl triplanar_ps, lit in the same pass with the deferred main light
// (kenshi::lighting). COLOURING and DUAL_TEXTURE use the mesh's vertex colours (VERTEX_COLORS).
// Samplers as StaticObject's texture units (objects.material): diffuseMap and normalMap
// trilinear with mipmap_bias -.5; the second maps, metalness and dust noise (made in code,
// F046402) Kenshi's default (anisotropic x16); the ambient map clamped.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view
#import kenshi::lighting::{apply_fog, main_light, sun_shadow, make_wet, reflection_dir, reflection_mip, to_screen}

struct ObjectParams {
    // xy: tiling; z: glossMult; w: alpha threshold (0: no TRANSPARENCY)
    tiling_gloss: vec4<f32>,
    // x: emissive brightness (0: none); y: double sided (1); z: wetness; w: 1 triplanar
    flags: vec4<f32>,
    sun_direction: vec4<f32>,
    sun_colour: vec4<f32>,
    env_colour: vec4<f32>,
    ambient_params: vec4<f32>,
    // xyz: the scene origin in world units; w: exposure
    origin_exposure: vec4<f32>,
    // x: world size
    world: vec4<f32>,
    sun_real: vec4<f32>,
    horizon: vec4<f32>,
    fog: vec4<f32>,
    fog_colour: vec4<f32>,
    // x: DUAL_TEXTURE (1); y: DUST (1); z: COLOURING (1); w: Grass (1)
    modes: vec4<f32>,
    // dustColour (the biome's ground colour)
    dust_colour: vec4<f32>,
    // dustAmount: amount, inside, stickiness
    dust_amount: vec4<f32>,
    // CONSTRUCTION: (constructionState, upperPos, scaffoldTiling, base height in the scene);
    // off while y is 0
    construction: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: ObjectParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var base_normal: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var metal_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var ambient_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var irradiance_cube: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var specularity_cube: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var cube_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var base_map2: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var base_normal2: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var metal_map2: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var dust_noise: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var aniso_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(14) var ambient_sampler: sampler;
// common/shadowFunctions.hlsl jitterMap (white-noise.png)
@group(#{MATERIAL_BIND_GROUP}) @binding(15) var shadow_jitter: texture_2d<f32>;
// CONSTRUCTION's scaffold
@group(#{MATERIAL_BIND_GROUP}) @binding(16) var grid_map: texture_2d<f32>;

const MAP_BIAS: f32 = -0.5;

// a texture unit's sample: the trilinear units (base_sampler) with their mip bias
fn unit(tex: texture_2d<f32>, s: sampler, uv: vec2<f32>, bias: f32) -> vec4<f32> {
    return textureSampleBias(tex, s, uv, bias);
}

fn triplanar(tex: texture_2d<f32>, s: sampler, bias: f32, coord: vec3<f32>, blend: vec3<f32>) -> vec4<f32> {
    let cx = unit(tex, s, coord.yz, bias);
    let cy = unit(tex, s, coord.xz, bias);
    let cz = unit(tex, s, coord.xy, bias);
    return cx * blend.x + cy * blend.y + cz * blend.z;
}

fn triplanar_normal(tex: texture_2d<f32>, s: sampler, bias: f32, coord: vec3<f32>, normal: vec3<f32>, blend: vec3<f32>) -> vec3<f32> {
    var nx = unit(tex, s, coord.yz, bias).brga;
    var ny = unit(tex, s, coord.xz, bias).rbga;
    var nz = unit(tex, s, coord.xy, bias).rgba;
    nx.r = mix(nx.r, 1.0 - nx.r, step(normal.x, 0.0));
    ny.g = mix(ny.g, 1.0 - ny.g, step(normal.y, 0.0));
    nz.b = mix(nz.b, 1.0 - nz.b, step(normal.z, 0.0));
    let n = nx * blend.x + ny * blend.y + nz * blend.z;
    return n.xyz * 2.0 - 1.0;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let origin = p.origin_exposure.xyz;
    let world_pos = in.world_position.xyz + origin;
#ifdef VERTEX_COLORS
    let colour = in.color;
#else
    let colour = vec4<f32>(1.0);
#endif
    let dual = p.modes.x > 0.5;
    let n = normalize(in.world_normal);
    var diffuse: vec4<f32>;
    var world_normal: vec3<f32>;
    var metalness: f32;
    var normal_a = 1.0;
    let is_triplanar = p.flags.w > 0.5;

    // both paths' samples (uniform control flow)
    // triplanar.hlsl: world coordinates / 5000 x tiling.xxy, blend by the normal
    let tcoord = world_pos / 5000.0 * vec3<f32>(p.tiling_gloss.x, p.tiling_gloss.x, p.tiling_gloss.y);
    var tblend = clamp(abs(n) - 0.2, vec3<f32>(0.0), vec3<f32>(1.0));
    tblend = max(vec3<f32>(0.0), pow(tblend * 7.0, vec3<f32>(2.0)));
    tblend = tblend / max(tblend.x + tblend.y + tblend.z, 1e-6);
    let t_diffuse = triplanar(base_map, base_sampler, MAP_BIAS, tcoord, tblend);
    let t_normal = triplanar_normal(base_normal, base_sampler, MAP_BIAS, tcoord, n, tblend);
    let t_metal = triplanar(metal_map, aniso_sampler, 0.0, tcoord, tblend).r;
    let t_diffuse2 = triplanar(base_map2, aniso_sampler, 0.0, tcoord, tblend);
    let t_normal2 = triplanar_normal(base_normal2, aniso_sampler, 0.0, tcoord, n, tblend);
    let t_metal2 = triplanar(metal_map2, aniso_sampler, 0.0, tcoord, tblend).r;
#ifdef VERTEX_UVS_A
    let uv = in.uv * p.tiling_gloss.xy;
#else
    let uv = vec2<f32>(0.0);
#endif
    // (enhancement, not Kenshi's) a cut-out keeps its coverage far away: its alpha raised by a
    // quarter per mip level the map is read at, so leaves and grass do not thin to specks
    let cut_size = vec2<f32>(textureDimensions(base_normal));
    let cut_dx = dpdx(uv * cut_size);
    let cut_dy = dpdy(uv * cut_size);
    let coverage = min(1.0 + max(0.0, 0.5 * log2(max(dot(cut_dx, cut_dx), dot(cut_dy, cut_dy)))) * 0.25, 1.6);
    let o_diffuse = unit(base_map, base_sampler, uv, MAP_BIAS);
    let o_normal = unit(base_normal, base_sampler, uv, MAP_BIAS);
    let o_metal = textureSample(metal_map, aniso_sampler, uv).r;
    let o_diffuse2 = textureSample(base_map2, aniso_sampler, uv);
    let o_normal2 = textureSample(base_normal2, aniso_sampler, uv);
    let o_metal2 = textureSample(metal_map2, aniso_sampler, uv).r;
    let noise = textureSample(dust_noise, aniso_sampler, world_pos.xz * 0.002);

    if (is_triplanar) {
        diffuse = t_diffuse;
        var sn = t_normal;
        metalness = t_metal;
        if (dual) {
            diffuse = mix(t_diffuse2, diffuse, colour.a);
            sn = mix(t_normal2, sn, colour.a);
            metalness = mix(t_metal2, metalness, colour.a);
        }
        world_normal = normalize(n + sn);
    } else {
        diffuse = o_diffuse;
        var normal_tex = o_normal;
        metalness = o_metal;
        if (dual) {
            diffuse = mix(o_diffuse2, diffuse, colour.a);
            normal_tex = mix(o_normal2, normal_tex, colour.a);
            metalness = mix(o_metal2, metalness, colour.a);
        }
        normal_a = normal_tex.a;
        // tangent-space normal (not DXT5NORMAL: xyz * 2 - 1)
        let nt = normal_tex.xyz * 2.0 - 1.0;
#ifdef VERTEX_TANGENTS
        let t = normalize(in.world_tangent.xyz);
        let b = normalize(cross(in.world_tangent.xyz, in.world_normal) * in.world_tangent.w);
        world_normal = normalize(nt.x * t + nt.y * b + nt.z * n);
#else
        world_normal = n;
#endif
    }
    if (p.modes.z > 0.5) {
        diffuse = vec4<f32>(diffuse.rgb * colour.rgb, diffuse.a);
    }
    if (p.flags.y > 0.5 && !front) {
        world_normal = -world_normal;
    }

    // Grass (foliage.hlsl grass_fs): the normal straight up, no gloss, the cut below
    let grass = p.modes.w > 0.5;
    if (grass) {
        world_normal = vec3<f32>(0.0, 1.0, 0.0);
        diffuse = vec4<f32>(diffuse.rgb, 0.0);
    }
    // CONSTRUCTION (objects.hlsl): the scaffold blended in, only its bars above 1.2 x the
    // progress of the height, cleared from the top over the last fifth
    let grid = textureSample(grid_map, base_sampler, in.uv * p.construction.z);
    if (p.construction.y > 0.0) {
        var grid_amount = grid.a;
        let height = (in.world_position.y - p.construction.w) / p.construction.y;
        let finishing = 1.0 - (p.construction.x - 0.8) * 5.0;
        if (p.construction.x * 1.2 < height && grid_amount - 0.7 < 0.0) {
            discard;
        }
        if (finishing < height) {
            grid_amount = 0.0;
        }
        diffuse = mix(diffuse, vec4<f32>(grid.rgb, 0.4), grid_amount);
        world_normal = normalize(mix(world_normal, n, grid_amount));
    }

    // TRANSPARENCY: alpha test on the normal map's alpha
    var translucency = 0.0;
    if (grass) {
        if (normal_a * coverage < p.tiling_gloss.w) {
            discard;
        }
    } else if (p.tiling_gloss.w > 0.0) {
        if (normal_a * coverage < p.tiling_gloss.w) {
            discard;
        }
        translucency = (1.0 - (normal_a - p.tiling_gloss.w) / (1.0 - p.tiling_gloss.w)) * 0.5 * 2.0;
    }

    // DUST
    if (p.modes.y > 0.5) {
        let noise_effect = 2.2 - noise.x * 6.0;
        let slope_effect = clamp((world_normal.y - 0.7 + p.dust_amount.z) * 4.0, 0.0, 1.0);
        let gloss_effect = -diffuse.a * 6.0;
        var dust = slope_effect + gloss_effect + noise_effect;
        dust = dust * 2.0 + p.dust_amount.x - 1.0;
        dust = dust * min(1.0, p.dust_amount.x * 8.0);
        diffuse = vec4<f32>(mix(diffuse.rgb, p.dust_colour.rgb, clamp(dust, 0.0, 1.0)), diffuse.a);
        if (!is_triplanar) {
            world_normal = normalize(mix(world_normal, n, clamp(dust * 0.5, 0.0, 1.0)));
        }
    }

    // wetness (objects.hlsl; triplanar.hlsl has none)
    diffuse.a = diffuse.a * p.tiling_gloss.z;
    if (grass) {
        // grass_fs: makeWet with absorbance 0.9, the gloss written x 0.6
        diffuse = make_wet(diffuse, p.flags.z, 0.9, 100.0 - world_pos.y, 0.5);
        diffuse.a = diffuse.a * 0.6;
    } else if (!is_triplanar) {
        var absorb = 1.0 - diffuse.a;
        absorb = absorb * (1.0 - metalness);
        diffuse = make_wet(diffuse, p.flags.z, absorb, 100.0 - world_pos.y, 0.5);
    }

    // EMISSIVE: buf1.a = 0.5 + brightness * (1 - normal.a) * 0.5; the light pass reads
    // max(0, a - 0.5) * 6.4
    var emissive = 0.0;
    if (p.flags.x > 0.0) {
        emissive = p.flags.x * (1.0 - normal_a) * 0.5 * 6.4;
    }

    let camera = view.world_position + origin;
    let view_dir = normalize(camera - world_pos);
    let world_size = p.world.x;
    let ambient_mult = textureSample(ambient_map, ambient_sampler, (world_pos.xz + world_size * 0.5) / world_size);
    // the G-buffer (8-bit, deferred.hlsl reads it back) holds the gloss and metalness clamped
    // to 0..1: a "specular mult" over 1 saturates rather than going past a mirror
    let gloss = clamp(diffuse.a, 0.0, 1.0);
    metalness = clamp(metalness, 0.0, 1.0);
    var irr = textureSampleLevel(irradiance_cube, cube_sampler, world_normal, 3.0);
    if (grass) {
        // (enhancement, not Kenshi's) the blades shade each other: the sky light falls from 0.7
        // at the tips to 0.25 at the roots (uv.y, 0 at the top), so the grass keeps its colour
        // instead of taking the whole sky as grass_fs's straight-up normal gives it
        irr = vec4<f32>(irr.rgb * mix(0.7, 0.25, clamp(uv.y, 0.0, 1.0)), irr.a);
    }
    let refl = textureSampleLevel(specularity_cube, cube_sampler,
        reflection_dir(world_normal, view_dir, gloss), reflection_mip(gloss));
    let lit = main_light(world_normal, view_dir, diffuse.rgb, gloss, metalness, emissive,
        translucency, p.sun_direction.xyz, p.sun_colour, p.env_colour.w, p.ambient_params.w,
        ambient_mult, irr, refl, sun_shadow(in.world_position, n, shadow_jitter));
    let fogged = apply_fog(lit, camera, world_pos, p.sun_real.xyz, p.horizon, p.fog, p.fog_colour,
        p.sun_colour.w);
    return vec4<f32>(to_screen(fogged, p.origin_exposure.w), 1.0);
}
