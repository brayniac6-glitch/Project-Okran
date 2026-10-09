// Kenshi's terrain, ported from data/materials/deferred/terrainfp4.hlsl (computeBiome, main_fs)
// and terrain.hlsl (main_vs texture coordinates), lit in the same pass with the deferred main
// light of deferred.hlsl (main_fs) and lightingFunctions.hlsl. Kenshi renders these as a G-buffer
// pass and a lighting pass; here they run in one forward pass with the same arithmetic.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view
#import kenshi::lighting::{apply_fog, main_light, sun_shadow, make_wet, reflection_dir, reflection_mip, to_screen}

struct TerrainParams {
    scales_a: array<vec4<f32>, 4>,
    scales_b: array<vec4<f32>, 4>,
    scales_c: array<vec4<f32>, 4>,
    slope_min: array<vec4<f32>, 4>,
    slope_max: array<vec4<f32>, 4>,
    slope_blend: array<vec4<f32>, 4>,
    overlay_mult: array<vec4<f32>, 4>,
    texture_fade: array<vec4<f32>, 4>,
    absorb0: array<vec4<f32>, 4>,
    absorb1: array<vec4<f32>, 4>,
    brightness_fix: vec4<f32>,
    distortion0: vec4<f32>,
    distortion1: vec4<f32>,
    // per biome: the blend map channel carrying its weight (0..3), 4: what the others leave
    channel: vec4<f32>,
    // x: biome count; yzw: the scene origin in world units
    count_origin: vec4<f32>,
    // the main light (deferred.hlsl main_fs uniforms)
    sun_direction: vec4<f32>,
    sun_colour: vec4<f32>,
    env_colour: vec4<f32>,
    ambient_params: vec4<f32>,
    // x: world size (units); y: wetness; z: exposure
    world: vec4<f32>,
    // the fog (post/fog.hlsl): sunDirectionReal, horizonClouds, pFogParams (w: the global fog's
    // density), fogColour
    sun_real: vec4<f32>,
    horizon: vec4<f32>,
    fog: vec4<f32>,
    fog_colour: vec4<f32>,
    // the blood on the ground: xy its corner (world x, z), z one over its width, w 1 when on
    blood: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: TerrainParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var overlay_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var linear_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var colour_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var blend_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var ambient_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var irradiance_cube: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var specularity_cube: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var cube_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var detail_sampler: sampler;
// deferred.material's ambientMap: clamped
@group(#{MATERIAL_BIND_GROUP}) @binding(58) var ambient_sampler: sampler;
// common/shadowFunctions.hlsl jitterMap (white-noise.png)
@group(#{MATERIAL_BIND_GROUP}) @binding(59) var shadow_jitter: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(60) var blood_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(61) var blood_sampler: sampler;
// four biomes x (six diffuse, six normal): bindings 10..57
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var d00: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var d01: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var d02: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var d03: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(14) var d04: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(15) var d05: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(16) var n00: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(17) var n01: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(18) var n02: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(19) var n03: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(20) var n04: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(21) var n05: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(22) var d10: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(23) var d11: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(24) var d12: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(25) var d13: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(26) var d14: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(27) var d15: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(28) var n10: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(29) var n11: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(30) var n12: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(31) var n13: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(32) var n14: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(33) var n15: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(34) var d20: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(35) var d21: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(36) var d22: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(37) var d23: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(38) var d24: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(39) var d25: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(40) var n20: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(41) var n21: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(42) var n22: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(43) var n23: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(44) var n24: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(45) var n25: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(46) var d30: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(47) var d31: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(48) var d32: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(49) var d33: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(50) var d34: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(51) var d35: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(52) var n30: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(53) var n31: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(54) var n32: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(55) var n33: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(56) var n34: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(57) var n35: texture_2d<f32>;


// The six layers of biome k (0 base, 1 slope, 2 cliff, 3 grass, 4 dirt, 5 road) at a uv.
fn diffuse(k: i32, layer: i32, uv: vec2<f32>) -> vec4<f32> {
    switch (k * 6 + layer) {
        case 0: { return textureSample(d00, detail_sampler, uv); }
        case 1: { return textureSample(d01, detail_sampler, uv); }
        case 2: { return textureSample(d02, detail_sampler, uv); }
        case 3: { return textureSample(d03, detail_sampler, uv); }
        case 4: { return textureSample(d04, detail_sampler, uv); }
        case 5: { return textureSample(d05, detail_sampler, uv); }
        case 6: { return textureSample(d10, detail_sampler, uv); }
        case 7: { return textureSample(d11, detail_sampler, uv); }
        case 8: { return textureSample(d12, detail_sampler, uv); }
        case 9: { return textureSample(d13, detail_sampler, uv); }
        case 10: { return textureSample(d14, detail_sampler, uv); }
        case 11: { return textureSample(d15, detail_sampler, uv); }
        case 12: { return textureSample(d20, detail_sampler, uv); }
        case 13: { return textureSample(d21, detail_sampler, uv); }
        case 14: { return textureSample(d22, detail_sampler, uv); }
        case 15: { return textureSample(d23, detail_sampler, uv); }
        case 16: { return textureSample(d24, detail_sampler, uv); }
        case 17: { return textureSample(d25, detail_sampler, uv); }
        case 18: { return textureSample(d30, detail_sampler, uv); }
        case 19: { return textureSample(d31, detail_sampler, uv); }
        case 20: { return textureSample(d32, detail_sampler, uv); }
        case 21: { return textureSample(d33, detail_sampler, uv); }
        case 22: { return textureSample(d34, detail_sampler, uv); }
        default: { return textureSample(d35, detail_sampler, uv); }
    }
}

fn normal_map(k: i32, layer: i32, uv: vec2<f32>) -> vec4<f32> {
    switch (k * 6 + layer) {
        case 0: { return textureSample(n00, detail_sampler, uv); }
        case 1: { return textureSample(n01, detail_sampler, uv); }
        case 2: { return textureSample(n02, detail_sampler, uv); }
        case 3: { return textureSample(n03, detail_sampler, uv); }
        case 4: { return textureSample(n04, detail_sampler, uv); }
        case 5: { return textureSample(n05, detail_sampler, uv); }
        case 6: { return textureSample(n10, detail_sampler, uv); }
        case 7: { return textureSample(n11, detail_sampler, uv); }
        case 8: { return textureSample(n12, detail_sampler, uv); }
        case 9: { return textureSample(n13, detail_sampler, uv); }
        case 10: { return textureSample(n14, detail_sampler, uv); }
        case 11: { return textureSample(n15, detail_sampler, uv); }
        case 12: { return textureSample(n20, detail_sampler, uv); }
        case 13: { return textureSample(n21, detail_sampler, uv); }
        case 14: { return textureSample(n22, detail_sampler, uv); }
        case 15: { return textureSample(n23, detail_sampler, uv); }
        case 16: { return textureSample(n24, detail_sampler, uv); }
        case 17: { return textureSample(n25, detail_sampler, uv); }
        case 18: { return textureSample(n30, detail_sampler, uv); }
        case 19: { return textureSample(n31, detail_sampler, uv); }
        case 20: { return textureSample(n32, detail_sampler, uv); }
        case 21: { return textureSample(n33, detail_sampler, uv); }
        case 22: { return textureSample(n34, detail_sampler, uv); }
        default: { return textureSample(n35, detail_sampler, uv); }
    }
}

struct Biome {
    albedo: vec4<f32>,
    normal: vec4<f32>,
    absorbance: f32,
}

// terrainfp4.hlsl computeBiome
fn compute_biome(k: i32, tc: vec3<f32>, blend: vec2<f32>, normal: vec3<f32>, weights: vec4<f32>,
                 map: vec4<f32>, colour: vec4<f32>, distance: f32) -> Biome {
    let s0 = p.scales_a[k];
    let s1 = p.scales_b[k];
    let s2 = p.scales_c[k];
    let overlay_mult = p.overlay_mult[k];
    let distant = p.texture_fade[k];

    var c_base = diffuse(k, 0, tc.xy * s1.xy);
    var c_slope = diffuse(k, 1, tc.xy * s0.xy);
    var c_grass = diffuse(k, 3, tc.xy * s1.zw);
    var c_dirt = diffuse(k, 4, tc.xy * s2.xy);
    var c_road = diffuse(k, 5, tc.xy * s2.zw);
    let c_cliff_x = diffuse(k, 2, tc.yz * s0.zw);
    let c_cliff_z = diffuse(k, 2, tc.xz * s0.zw);
    var c_cliff = c_cliff_x * blend.x + c_cliff_z * blend.y;

    let n_base = normal_map(k, 0, tc.xy * s1.xy);
    let n_slope = normal_map(k, 1, tc.xy * s0.xy);
    let n_grass = normal_map(k, 3, tc.xy * s1.zw);
    let n_dirt = normal_map(k, 4, tc.xy * s2.xy);
    let n_road = normal_map(k, 5, tc.xy * s2.zw);
    var n_cliff_x = normal_map(k, 2, tc.yz * s0.zw);
    var n_cliff_z = normal_map(k, 2, tc.xz * s0.zw);
    if (normal.x <= 0.0) { n_cliff_x.g = 1.0 - n_cliff_x.g; }
    if (normal.z >= 0.0) { n_cliff_z.g = 1.0 - n_cliff_z.g; }
    n_cliff_x.r = 1.0 - n_cliff_x.r;
    n_cliff_z.r = 1.0 - n_cliff_z.r;
    let n_cliff = n_cliff_x * blend.x + n_cliff_z * blend.y;

    let white = vec4<f32>(1.0);
    c_base = c_base * colour;
    c_slope = c_slope * colour;
    c_cliff = c_cliff * mix(white, colour, overlay_mult.x);
    c_grass = c_grass * mix(white, colour, overlay_mult.y);
    c_dirt = c_dirt * mix(white, colour, overlay_mult.z);
    c_road = c_road * mix(white, colour, overlay_mult.w);
    let fadeout = clamp(distance * distant.a - 0.3, 0.0, 1.0);

    var albedo = mix(c_base, c_grass, map.r);
    albedo = mix(albedo, c_slope, weights.x);
    albedo = mix(albedo, c_dirt, map.b);
    albedo = vec4<f32>(mix(albedo.rgb, distant.rgb, fadeout), albedo.a);
    albedo = vec4<f32>(mix(albedo.rgb, c_road.rgb, map.a), albedo.a);
    albedo = mix(albedo, c_cliff, weights.y);

    let a0 = p.absorb0[k];
    let a1 = p.absorb1[k];
    var absorb = mix(a0.x, a0.w, map.r);
    absorb = mix(absorb, a0.y, weights.x);
    absorb = mix(absorb, a1.x, map.b);
    absorb = mix(absorb, a1.y, map.a);
    absorb = mix(absorb, a0.z, weights.y);

    var n = mix(n_base, n_grass, map.r);
    n = mix(n, n_slope, weights.x);
    n = mix(n, n_dirt, map.b);
    n = mix(n, vec4<f32>(0.5, 0.5, 1.0, 1.0), fadeout);
    n = mix(n, n_road, map.a);
    n = mix(n, n_cliff, weights.y);

    var out: Biome;
    out.albedo = albedo;
    out.normal = n;
    out.absorbance = absorb;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let origin = p.count_origin.yzw;
    let world_pos = in.world_position.xyz + origin;
    let camera = view.world_position + origin;
    let normal = normalize(in.world_normal);

    // DistantTerrain (terrain.hlsl simple_fs), past the material distance: distant.png's colour
    // over the world, gloss 0.2, the normal clamped to positive components
    if (p.world.w > 0.5) {
        let ws = p.world.x;
        let colour = textureSample(colour_map, linear_sampler, (world_pos.xz + ws * 0.5) / ws).rgb;
        let dn = normalize(clamp(normal, vec3<f32>(0.0), vec3<f32>(1.0)) + vec3<f32>(0.0, 1e-4, 0.0));
        let dmap = (world_pos.xz + ws * 0.5) / ws;
        let dambient = textureSample(ambient_map, ambient_sampler, dmap);
        let dview = normalize(camera - world_pos);
        let dirr = textureSampleLevel(irradiance_cube, cube_sampler, dn, 3.0);
        let drefl = textureSampleLevel(specularity_cube, cube_sampler,
            reflection_dir(dn, dview, 0.2), reflection_mip(0.2));
        let dlit = main_light(dn, dview, colour, 0.2, 0.0, 0.0, 0.0,
            p.sun_direction.xyz, p.sun_colour, p.env_colour.w, p.ambient_params.w,
            dambient, dirr, drefl, sun_shadow(in.world_position, in.world_normal, shadow_jitter));
        let dfog = apply_fog(dlit, camera, world_pos, p.sun_real.xyz, p.horizon, p.fog, p.fog_colour,
            p.sun_colour.w);
        return vec4<f32>(to_screen(dfog, p.world.z), 1.0);
    }

    // terrain.hlsl main_vs: texture coordinates and cliff blending
    var tc = vec3<f32>(world_pos.xz / 5000.0, 1.0 - world_pos.y / 5000.0);
    let tv = vec4<f32>(tc.z);
    var blend = abs(normalize(normal.xz)) - 0.2;
    blend = max(vec2<f32>(0.0), pow(blend * 7.0, vec2<f32>(2.0)));
    blend = blend / (blend.x + blend.y);
    if (normal.y > 0.995) { blend = vec2<f32>(0.5, 0.5); }
    let wave = vec4<f32>(
        (cos(world_pos.x * p.distortion0.x) + cos(world_pos.z * p.distortion0.x)) * p.distortion0.y,
        (cos(world_pos.x * p.distortion0.z) + cos(world_pos.z * p.distortion0.z)) * p.distortion0.w,
        (cos(world_pos.x * p.distortion1.x) + cos(world_pos.z * p.distortion1.x)) * p.distortion1.y,
        (cos(world_pos.x * p.distortion1.z) + cos(world_pos.z * p.distortion1.z)) * p.distortion1.w,
    );

    // overlay data
    // a skirt's UVs are flagged (-1 - uv, kenshi_render::terrain_lod)
    let skirt = in.uv.x < -0.5;
    var uv = select(in.uv, -in.uv - 1.0, skirt);
    // a map feature (material type 2, kenshi_render::zone spawn_features; Feature_VP): the
    // overlay coordinates from the world position, in the colour tile uv_b.y - 1 names
#ifdef VERTEX_UVS_B
    if (in.uv_b.y > 0.5) {
        let code = u32(in.uv_b.y - 0.5);
        let tile = vec2<f32>(f32(code % 8u), f32(code / 8u));
        uv = (world_pos.xz + p.world.x * 0.5) / p.world.x * 8.0 - tile;
    }
#endif
    var map = textureSample(overlay_map, linear_sampler, uv);
    let colour = textureSample(colour_map, linear_sampler, uv) * 1.2;
    map.r = max(map.r, map.g);
    let distance = length(world_pos - camera);
    let slope = 1.0 - normal.y;

    // the biomes, weighted by the blend map (slots 0..3 in R, G, B, A; slot 4 the rest)
    let world_size = p.world.x;
    let bw = textureSample(blend_map, linear_sampler, (world_pos.xz + world_size * 0.5) / world_size);
    let rest = clamp(1.0 - bw.r - bw.g - bw.b - bw.a, 0.0, 1.0);
    var channels = array<f32, 5>(bw.r, bw.g, bw.b, bw.a, rest);
    let count = i32(p.count_origin.x);
    var albedo = vec4<f32>(0.0);
    var nrm = vec4<f32>(0.0);
    var absorbance = 0.0;
    var total = 0.0;
    for (var k = 0; k < 4; k = k + 1) {
        if (k >= count) { break; }
        let w = channels[i32(p.channel[k])];
        if (w <= 0.001 && count > 1) { continue; }
        tc.z = tv.x + wave[k];
        let weights = smoothstep(p.slope_min[k] - p.slope_blend[k], p.slope_min[k], vec4<f32>(slope))
            * smoothstep(p.slope_max[k] + p.slope_blend[k], p.slope_max[k], vec4<f32>(slope));
        var b = compute_biome(k, tc, blend, normal, weights, map, colour, distance);
        b.albedo = vec4<f32>(b.albedo.rgb * p.brightness_fix[k], b.albedo.a);
        let ww = select(w, 1.0, count == 1);
        albedo = albedo + b.albedo * ww;
        nrm = nrm + b.normal * ww;
        absorbance = absorbance + b.absorbance * ww;
        total = total + ww;
    }
    if (total > 0.0) {
        albedo = albedo / total;
        nrm = nrm / total;
        absorbance = absorbance / total;
    }

    // wetness: the weather's (SharedWaterParams wetness), and under the water (height 100)
    let absorb = 1.0 - albedo.a + absorbance;
    albedo = make_wet(albedo, p.world.y, absorb, 100.0 - world_pos.y, 2.0);

    // the blood on the ground (deferred/blood.hlsl terrain: the blood map over the area, alpha
    // blended into the G-buffer's albedo)
    if (p.blood.w > 0.0) {
        let buv = (world_pos.xz - p.blood.xy) * p.blood.z;
        if (all(buv >= vec2<f32>(0.0)) && all(buv <= vec2<f32>(1.0))) {
            let b = textureSampleLevel(blood_map, blood_sampler, buv, 0.0);
            albedo = vec4<f32>(mix(albedo.rgb, b.rgb, b.a), albedo.a);
        }
    }

    // the water reflection's view (kenshi_render::reflection: the only camera under the water):
    // its near plane at the water (harness stand-in: the discard), the G-buffer's first target
    // (gbuffer.hlsl writeAlbedo: the albedo's luma, YCoCg Y, in red)
    if (camera.y < 100.0) {
        if (world_pos.y < 100.0 || skirt) {
            discard;
        }
        let luma = 0.25 * albedo.r + 0.5 * albedo.g + 0.25 * albedo.b;
        return vec4<f32>(luma, 0.0, 0.0, albedo.a);
    }

    // to world space
    let binormal = normalize(cross(normal, vec3<f32>(-1.0, 0.0, 0.0)));
    let tangent = normalize(cross(binormal, normal));
    let tn = nrm.rgb * 2.0 - 1.0;
    let world_normal = normalize(tn.x * tangent + tn.y * binormal + tn.z * normal);

    // deferred.hlsl main_fs: sun and ambient
    let gloss = clamp(albedo.a, 0.0, 1.0);
    let map_uv = (world_pos.xz + world_size * 0.5) / world_size;
    let ambient_mult = textureSample(ambient_map, ambient_sampler, map_uv);
    let view_dir = normalize(camera - world_pos);
    let irr = textureSampleLevel(irradiance_cube, cube_sampler, world_normal, 3.0);
    let refl = textureSampleLevel(specularity_cube, cube_sampler,
        reflection_dir(world_normal, view_dir, gloss), reflection_mip(gloss));
    let lit = main_light(world_normal, view_dir, albedo.rgb, gloss, 0.0, 0.0, 0.0,
        p.sun_direction.xyz, p.sun_colour, p.env_colour.w, p.ambient_params.w,
        ambient_mult, irr, refl, sun_shadow(in.world_position, in.world_normal, shadow_jitter));
    let fogged = apply_fog(lit, camera, world_pos, p.sun_real.xyz, p.horizon, p.fog, p.fog_colour,
        p.sun_colour.w);
    return vec4<f32>(to_screen(fogged, p.world.z), 1.0);
}
