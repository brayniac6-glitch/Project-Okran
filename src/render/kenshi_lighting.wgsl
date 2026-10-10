// Kenshi's main light, ported from data/materials/deferred/deferred.hlsl (main_fs) and
// common/lightingFunctions.hlsl, plus common/wet.hlsl and the HDR composite's output
// (post/hdrfp4.hlsl Composite_fp4). Shared by the terrain and object materials.
#define_import_path kenshi::lighting

#import bevy_pbr::mesh_view_bindings as view_bindings
#import bevy_pbr::mesh_view_types
#import bevy_pbr::clustered_forward as clustering

const PI: f32 = 3.14159265;
const DIELECTRIC_SPEC: f32 = 0.04;

struct Light {
    diffuse: vec3<f32>,
    specular: vec3<f32>,
}

fn fresnel_diffuse(spec: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(clamp(1.0 - dot(spec, vec3<f32>(1.0 / 3.0)), 0.0, 1.0));
}

// LightingFuncGGX_OPT3
fn ggx(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32, f0: vec3<f32>) -> vec3<f32> {
    let h = normalize(v + l);
    let dot_nl = clamp(dot(n, l), 0.0, 1.0);
    let dot_lh = clamp(dot(l, h), 0.0, 1.0);
    let dot_nh = clamp(dot(n, h), 0.0, 1.0);
    let alpha = roughness * roughness;
    let alpha_sqr = alpha * alpha;
    let denom = max(1e-6, dot_nh * dot_nh * (alpha_sqr - 1.0) + 1.0);
    let d = alpha_sqr / (PI * denom * denom);
    let dot_lh5 = exp2((-5.55473 * dot_lh - 6.98316) * dot_lh);
    let k = alpha / 2.0;
    let k2 = k * k;
    let vis = 1.0 / (dot_lh * dot_lh * (1.0 - k2) + k2);
    let fv = f0 * vis + (1.0 - f0) * dot_lh5 * vis;
    return dot_nl * d * fv;
}

fn gloss_to_roughness(gloss: f32) -> f32 {
    return 1.0 - gloss * 0.99;
}

// CalcPunctualLight
fn punctual(n: vec3<f32>, l: vec3<f32>, v: vec3<f32>, gloss: f32, spec: vec3<f32>, colour: vec3<f32>,
            translucency: f32) -> Light {
    let roughness = gloss_to_roughness(gloss);
    var dot_nl = dot(l, n);
    if (dot_nl < 0.0) {
        dot_nl = mix(0.0, -dot_nl, translucency);
    } else {
        dot_nl = clamp(dot_nl, 0.0, 1.0);
    }
    let factor = PI * dot_nl * colour;
    var out: Light;
    out.diffuse = factor * fresnel_diffuse(spec);
    out.specular = colour * ggx(n, v, l, roughness, spec) / PI;
    return out;
}

// GetSpecularDominantDir
fn specular_dominant_dir(n: vec3<f32>, r: vec3<f32>, gloss: f32) -> vec3<f32> {
    let roughness = gloss_to_roughness(gloss);
    let f = gloss * (sqrt(gloss) + roughness);
    return mix(n, r, f);
}

// LazarovEnvironmentBRDF
fn lazarov(gloss: f32, nov: f32, rf0: vec3<f32>) -> vec3<f32> {
    var t = vec4<f32>(1.0 / 0.96, 0.475, (0.0275 - 0.25 * 0.04) / 0.96, 0.25);
    t = t * gloss;
    t = t + vec4<f32>(0.0, 0.0, (0.015 - 0.75 * 0.04) / 0.96, 0.75);
    let a0 = t.x * min(t.y, exp2(-9.28 * nov)) + t.z;
    let a1 = t.w;
    return clamp(a0 + rf0 * (a1 - a0), vec3<f32>(0.0), vec3<f32>(1.0));
}

// The direction to sample the specularity cube at, and its mip level (CalcEnvironmentLight).
fn reflection_dir(n: vec3<f32>, v: vec3<f32>, gloss: f32) -> vec3<f32> {
    return specular_dominant_dir(n, -reflect(v, n), gloss);
}
fn reflection_mip(gloss: f32) -> f32 {
    return (1.0 - gloss) * 7.0;
}

// deferred.hlsl main_fs: the sun and the environment on a surface. `irradiance` and
// `reflection` are the two cubes' samples (rgba, decoded here); `ambient_mult` the ambient map.
fn main_light(
    normal: vec3<f32>, view_dir: vec3<f32>, albedo_in: vec3<f32>, gloss: f32, metalness: f32,
    emissive: f32, translucency: f32,
    sun_direction: vec3<f32>, sun_colour: vec4<f32>, env_colour_w: f32, ambient_w: f32,
    ambient_mult: vec4<f32>, irradiance: vec4<f32>, reflection: vec4<f32>, shadow: f32,
) -> vec3<f32> {
    let sun = sun_colour.rgb * sun_colour.w * ambient_w * ambient_mult.w * 2.0 * shadow;
    let light_dir = normalize(sun_direction);
    let spec = mix(vec3<f32>(DIELECTRIC_SPEC), albedo_in, metalness);
    let albedo = mix(albedo_in, vec3<f32>(0.0), metalness);
    let sun_light = punctual(normal, light_dir, view_dir, gloss, spec, sun, translucency);
    // CalcEnvironmentLight
    let nov = clamp(dot(view_dir, normal), 0.0, 1.0);
    var env: Light;
    env.specular = lazarov(gloss, nov, spec) * (reflection.rgb * reflection.a * 10.0);
    env.diffuse = (irradiance.rgb * irradiance.a * 4.0) * fresnel_diffuse(spec);
    let env_w = env_colour_w * clamp(light_dir.y * 5.0 + 0.2, 0.1, 1.0);
    env.diffuse = env.diffuse * ambient_mult.rgb * env_w;
    env.specular = env.specular * ambient_mult.rgb * env_w;
    return sun_light.specular + env.specular + (sun_light.diffuse + env.diffuse) * albedo
        + albedo * emissive;
}

// deferred.hlsl light_fs: the lamps, torches and fires near the surface (Bevy's clustered
// point and spot lights, made from Kenshi's LIGHT records by local_lights.rs). Each lights
// what faces it within its radius, falling off by Kenshi's curve: (1 - x)^3 x 0.8 + 0.2 to
// x = 0.649, then -3 (x - 0.6)^2 + 0.242 to 0.8, then 3 (x - 1)^2 (x the distance over the
// radius); a spot also by its cone ((a - cos outer) / (cos inner - cos outer), its "falloff"
// taken as 1). `scene_pos` is the Bevy world position (the scene's), `frag_coord` the pixel's.
fn local_lights(scene_pos: vec3<f32>, frag_coord: vec2<f32>, normal: vec3<f32>, view_dir: vec3<f32>,
                albedo_in: vec3<f32>, gloss: f32, metalness: f32) -> vec3<f32> {
    let view_z = dot(vec4<f32>(
        view_bindings::view.view_from_world[0].z,
        view_bindings::view.view_from_world[1].z,
        view_bindings::view.view_from_world[2].z,
        view_bindings::view.view_from_world[3].z
    ), vec4<f32>(scene_pos, 1.0));
    let cluster = clustering::view_fragment_cluster_index(frag_coord, view_z, false);
    let ranges = clustering::unpack_clusterable_object_index_ranges(cluster);
    let spec = mix(vec3<f32>(DIELECTRIC_SPEC), albedo_in, metalness);
    let albedo = mix(albedo_in, vec3<f32>(0.0), metalness);
    var out = vec3<f32>(0.0);
    for (var i: u32 = ranges.first_point_light_index_offset; i < ranges.first_reflection_probe_index_offset; i = i + 1u) {
        let id = clustering::get_clusterable_object_id(i);
        let light = &view_bindings::clustered_lights.data[id];
        let to_light = (*light).position_radius.xyz - scene_pos;
        let dist = length(to_light);
        let radius = inverseSqrt(max((*light).color_inverse_square_range.w, 1e-12));
        if (dist >= radius) {
            continue;
        }
        let l = to_light / max(dist, 1e-4);
        if (dot(normal, l) < 0.01) {
            continue;
        }
        let x = clamp(dist / radius, 0.0, 1.0);
        let start = pow(1.0 - x, 3.0) * 0.8 + 0.2;
        let vb = -3.0 * pow(x - 0.6, 2.0) + 0.242;
        let vc = 3.0 * pow(x - 1.0, 2.0);
        var attenuation = select(vb, start, x < 0.649);
        attenuation = select(vc, attenuation, x < 0.8);
        if (i >= ranges.first_spot_light_index_offset) {
            var dir = vec3<f32>((*light).light_custom_data.x, 0.0, (*light).light_custom_data.y);
            dir.y = sqrt(max(0.0, 1.0 - dir.x * dir.x - dir.z * dir.z));
            if (((*light).flags & mesh_view_types::POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE) != 0u) {
                dir.y = -dir.y;
            }
            let a = dot(dir, -l);
            attenuation = attenuation * clamp(a * (*light).light_custom_data.z + (*light).light_custom_data.w, 0.0, 1.0);
        }
        let lit = punctual(normal, l, view_dir, gloss, spec, (*light).color_inverse_square_range.rgb, 0.0);
        out = out + (lit.specular + lit.diffuse * albedo) * attenuation;
    }
    return out;
}

// wet.hlsl makeWet
fn make_wet(fragment: vec4<f32>, amount: f32, absorbance: f32, water_height: f32, water_edge: f32) -> vec4<f32> {
    let underwater = clamp((water_height + water_edge) / water_edge, 0.0, 1.0);
    let wet = min(amount + underwater, 1.0);
    var darken = (1.0 - 1.0 / (wet + 0.7)) * absorbance;
    var shine = 2.0 * pow(max(0.0, wet - 0.15 * absorbance), 1.0 + 4.0 * absorbance);
    darken = min(darken + underwater * 0.2, 0.4);
    shine = shine * clamp((1.0 - underwater) * 100.0, 0.0, 1.0);
    let rgb = fragment.rgb * (1.0 - darken * 0.5);
    return vec4<f32>(rgb, mix(fragment.a, 0.5, clamp(shine, 0.0, 1.0)));
}

// The scene buffer takes raw HDR; the HDR chain (kenshi_hdr.wgsl) exposes it afterwards.
fn to_screen(c: vec3<f32>, exposure: f32) -> vec3<f32> {
    return c;
}

// post/atmospherefog.hlsl calculateAtmosphereFog with the SkyX uniforms Kenshi's atmosphere
// options give (F040804: inner radius 9.77501, outer 10.2963, height 0.01, Rayleigh 0.0022,
// Mie 0.000675, sun intensity 30, wavelengths (0.57, 0.48, 0.44), exposure 1.4).
const SKY_INNER: f32 = 9.77501;
const SKY_OUTER: f32 = 10.2963;
const SKY_HEIGHT: f32 = 0.01;
const SKY_KR: f32 = 0.0022;
const SKY_KM: f32 = 0.000675;
const SKY_ESUN: f32 = 30.0;
const SKY_EXPOSURE: f32 = 1.4;
const FOG_SAMPLES: i32 = 4;

fn sky_scale(coss: f32, scale_depth: f32) -> f32 {
    let x = 1.0 - coss;
    return scale_depth * exp(-0.00287 + x * (0.459 + x * (3.83 + x * (-6.80 + x * 5.25))));
}

// `horizon`: horizonClouds; `fog_params`: pFogParams (y start, z end); `skydome_radius`:
// uSkydomeRadius. Returns the fog colour and its blend.
fn atmosphere_fog(camera_pos: vec3<f32>, world_pos: vec3<f32>, distance: f32, light_dir: vec3<f32>,
                  horizon: vec4<f32>, fog_params: vec4<f32>, skydome_radius: f32) -> vec4<f32> {
    let inv_wl = 1.0 / pow(vec3<f32>(0.57, 0.48, 0.44), vec3<f32>(4.0));
    let kr_esun = SKY_KR * SKY_ESUN;
    let kr4pi = SKY_KR * 4.0 * PI;
    let km4pi = SKY_KM * 4.0 * PI;
    let scale = 1.0 / (SKY_OUTER - SKY_INNER);
    let scale_depth = (SKY_OUTER - SKY_INNER) * 0.5;
    let scale_over_depth = scale / scale_depth;
    let cam = vec3<f32>(0.0, SKY_INNER + (SKY_OUTER - SKY_INNER) * SKY_HEIGHT, 0.0);

    var pos = (world_pos - camera_pos) / skydome_radius;
    pos.y = pos.y + SKY_INNER;
    let l = pos.x * pos.x + pos.z * pos.z;
    pos.y = mix(max(pos.y, SKY_INNER), pos.y, clamp(l, 0.0, 1.0));

    var ray = pos - cam;
    var far = length(ray);
    ray = ray / far;
    far = clamp(far, 0.0, 1.0);
    if (ray.y < -0.3) {
        ray = vec3<f32>(0.0, -0.3, 0.953);
    }
    let start = cam;
    let height0 = cam.y;
    let start_angle = dot(ray, start) / height0;
    let depth0 = exp(scale_over_depth * (SKY_INNER - cam.y));
    let start_offset = depth0 * sky_scale(start_angle, scale_depth);

    let sample_length = far / f32(FOG_SAMPLES);
    let scaled_length = sample_length * scale;
    let sample_ray = ray * sample_length;
    var point = start + sample_ray * 0.5;
    var colour = vec3<f32>(0.0);
    for (var i = 0; i < FOG_SAMPLES; i = i + 1) {
        let height = length(point);
        let depth = exp(scale_over_depth * (SKY_INNER - height));
        let light_angle = dot(light_dir, point) / height;
        let camera_angle = dot(ray, point) / height;
        let scatter = start_offset + depth * (sky_scale(light_angle, scale_depth) - sky_scale(camera_angle, scale_depth));
        let attenuate = exp(-scatter * (inv_wl * kr4pi + km4pi)) * (depth * scaled_length);
        colour = colour + attenuate;
        point = point + sample_ray;
    }
    let rayleigh = colour * (inv_wl * kr_esun);
    let direction = cam - pos;
    let coss = -dot(light_dir, direction) / length(direction);
    let phase = 0.75 * (1.0 + 0.5 * coss * coss);
    var c = phase * rayleigh * SKY_EXPOSURE;
    c = mix(c, horizon.rgb, horizon.a);
    let level = clamp((distance - fog_params.y) / (fog_params.z - fog_params.y), 0.0, 1.0);
    return vec4<f32>(c, level);
}

// post/fog.hlsl fogValue: the global (weather) fog.
fn fog_value(depth: f32, density: f32, colour: vec4<f32>, light: f32) -> vec4<f32> {
    let amount = clamp(depth * density, 0.0, 1.0);
    var curve: f32;
    if (amount < 0.5) {
        curve = 2.0 * amount * amount;
    } else {
        curve = -2.0 * (amount - 1.0) * (amount - 1.0) + 1.0;
    }
    return vec4<f32>(colour.rgb * light, curve * colour.a);
}

// post/fog.hlsl atmosphere_fog_fs on a surface (not the far plane): the atmosphere fog under the
// global fog, alpha blended over the lit colour. `fog_params.w` is the global fog's density,
// `fog_colour` its colour; `sun_w` the sun colour's strength.
fn apply_fog(lit: vec3<f32>, camera_pos: vec3<f32>, world_pos: vec3<f32>, sun_real: vec3<f32>,
             horizon: vec4<f32>, fog_params: vec4<f32>, fog_colour: vec4<f32>, sun_w: f32) -> vec3<f32> {
    let distance = length(world_pos - camera_pos);
    // uSkydomeRadius: SkyX's skydome at its radius multiplier (0.95) of the far clip, which
    // Kenshi sets to pFogParams.x (view distance x 10, F041473)
    let atmosphere = atmosphere_fog(camera_pos, world_pos, distance, sun_real, horizon, fog_params,
        fog_params.x * 0.95);
    let global = fog_value(distance, fog_params.w, fog_colour, sun_w);
    var r = mix(atmosphere, global, global.a);
    r.a = clamp(atmosphere.a + global.a, 0.0, 1.0);
    return mix(lit, r.rgb, r.a);
}

// The sun's shadow on a surface (deferred.hlsl multiplies the sun's colour by it): Kenshi's CSM
// lookup (common/shadowFunctions.hlsl computeShadowMultiplier: PCF HEX12 with the jitter map; the
// default "shadow mode" 1) over the first directional light's four cascades. The cascades are
// Kenshi's (F046650's splits, put on the light by kenshi_render::shadows), fitted and drawn by
// Bevy, whose fit is F046651's (the larger of the slice's body and far-plane diagonals, ceiled,
// the centre snapped to texels). The cascade's viewport of Kenshi's 2 x 2 atlas is one layer here
// (atlas uv offsets x 2).
// Harness stand-ins: the casters' bias (shadowCasterFragProg, biasParams from F046651) is applied
// here at the receiver, its slope from the receiver's tilt to the light; a cascade's depth extent
// is taken as its diameter (F046651 ceils the light-space box to it); samples past the cascade's
// edge clamp to it (Kenshi's run into the next viewport).
// Enhancements (not Kenshi's): each of the twelve taps is a bilinear hardware comparison (Kenshi
// reads one texel), and the last tenth of a cascade blends into the next (Kenshi switches hard).
fn sun_shadow(world_position: vec4<f32>, normal: vec3<f32>, jitter: texture_2d<f32>) -> f32 {
    if (view_bindings::lights.n_directional_lights == 0u) {
        return 1.0;
    }
    let flags = view_bindings::lights.directional_lights[0].flags;
    if ((flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u) {
        return 1.0;
    }
    let count = view_bindings::lights.directional_lights[0].num_cascades;
    if (count == 0u) {
        return 1.0;
    }
    // the fragment's depth (posSs.z) against the splits (csmParams[i].x); past the last, lit
    let depth = -dot(vec4<f32>(
        view_bindings::view.view_from_world[0].z,
        view_bindings::view.view_from_world[1].z,
        view_bindings::view.view_from_world[2].z,
        view_bindings::view.view_from_world[3].z
    ), world_position);
    if (depth > view_bindings::lights.directional_lights[0].cascades[count - 1u].far_bound) {
        return 1.0;
    }
    var c = 0u;
    for (var i = 0u; i + 1u < count; i = i + 1u) {
        if (depth > view_bindings::lights.directional_lights[0].cascades[i].far_bound) {
            c = i + 1u;
        }
    }
    let shadow = cascade_shadow(c, world_position, normal, jitter);
    // (enhancement) the seam: the last tenth of the cascade fades into the next
    let far = view_bindings::lights.directional_lights[0].cascades[c].far_bound;
    // (the first cascade from the minimum distance, 1)
    var near = 1.0;
    if (c > 0u) {
        near = view_bindings::lights.directional_lights[0].cascades[c - 1u].far_bound;
    }
    let fade = clamp((depth - (far - (far - near) * 0.1)) / max((far - near) * 0.1, 1e-4), 0.0, 1.0);
    if (fade > 0.0 && c + 1u < count) {
        return mix(shadow, cascade_shadow(c + 1u, world_position, normal, jitter), fade);
    }
    if (fade > 0.0) {
        // the last cascade fades to lit, as past it
        return mix(shadow, 1.0, fade);
    }
    return shadow;
}

// One cascade's HEX12 lookup (computeShadowMultiplier's PCF part).
fn cascade_shadow(c: u32, world_position: vec4<f32>, normal: vec3<f32>, jitter: texture_2d<f32>) -> f32 {
    let m = view_bindings::lights.directional_lights[0].cascades[c].clip_from_world;
    let r0 = vec3<f32>(m[0].x, m[1].x, m[2].x);
    let r1 = vec3<f32>(m[0].y, m[1].y, m[2].y);
    let r2 = vec3<f32>(m[0].z, m[1].z, m[2].z);
    // 2 / the cascade's diameter; the depth per unit
    let scale = length(r0);
    let diameter = 2.0 / scale;
    let per_unit = length(r2);
    let clip = m * world_position;
    let uv = clip.xy * vec2<f32>(0.5, -0.5) + 0.5;
    let texels = f32(textureDimensions(view_bindings::directional_shadow_textures).x);
    let layer = i32(view_bindings::lights.directional_lights[0].depth_texture_base_index + c);

    // all axes flattened into the plane of the normal (in light space)
    let nls = normalize(vec3<f32>(dot(r0, normal) / scale, dot(r1, normal) / scale, dot(r2, normal) / per_unit));
    let x_dir = vec3<f32>(1.0, 0.0, 0.0) - nls.x * nls;
    let y_dir = vec3<f32>(0.0, 1.0, 0.0) - nls.y * nls;
    let z_dir = vec3<f32>(0.0, 0.0, 1.0) - nls.z * nls;

    // csmParams[c].y, the filter radius (F046651): max(|scale row 0| x 0.3, 0.8 / viewport width)
    let radius = max(scale * 0.3, 0.8 / texels);
    // the jitter map (filtering none, wrapping) at the atlas uv x 1024: the viewport's corner
    // (a multiple of 0.5) drops out
    let jd = vec2<i32>(textureDimensions(jitter));
    let jt = vec2<i32>(floor(uv * 0.5 * 1024.0 * vec2<f32>(jd)));
    let noise = textureLoad(jitter, ((jt % jd) + jd) % jd, 0).xy;
    let ang = noise.x * 2.0 * 3.1415;
    let sa = sin(ang);
    let ca = cos(ang);
    // max offset radius is 2
    let k = radius * 0.6 * 0.5;
    let offs = vec2<f32>(noise.x * 0.25, 0.0);

    // the casters' bias in depth units (1 / the depth extent): fixed max(|scale[2][2]| x 0.75,
    // 0.001); slope min(0.04, 4 x the depth's change per shadow texel)
    let tilt = length(nls.xy) / max(abs(nls.z), 1e-4);
    let bias = (max(scale * 0.75, 0.001) + min(0.04, 4.0 * tilt / texels)) * diameter * per_unit;

    // HEX12 (its second and third offsets are the same in Kenshi's table)
    var hex = array<vec2<f32>, 12>(
        vec2<f32>(1.0, 0.0),
        vec2<f32>(-0.5, 0.866025),
        vec2<f32>(-0.5, 0.866025),
        vec2<f32>(2.5, 0.866025),
        vec2<f32>(1.0, 2.0 * 0.866025),
        vec2<f32>(-0.5, 3.0 * 0.866025),
        vec2<f32>(-2.0, 2.0 * 0.866025),
        vec2<f32>(-2.0, 0.0),
        vec2<f32>(-2.0, -2.0 * 0.866025),
        vec2<f32>(-0.5, -3.0 * 0.866025),
        vec2<f32>(1.0, -2.0 * 0.866025),
        vec2<f32>(2.5, -0.866025),
    );
    let edge = vec2<f32>(0.5 / texels);
    var lit = 0.0;
    for (var i = 0; i < 12; i = i + 1) {
        let t = hex[i] + offs;
        let v = vec3<f32>(ca * t.x + sa * t.y, -sa * t.x + ca * t.y, 0.0) * k;
        let o = vec3<f32>(dot(x_dir, v), dot(y_dir, v), dot(z_dir, v));
        // o is in atlas uv and Kenshi's depth (growing away from the light; Bevy's toward it)
        let suv = clamp(uv + o.xy * 2.0, edge, 1.0 - edge);
        let sdepth = clip.z - o.z * diameter * per_unit;
        // (enhancement) bilinear comparison: lit where the receiver is at least as near the light
        lit = lit + textureSampleCompareLevel(
            view_bindings::directional_shadow_textures,
            view_bindings::directional_shadow_textures_comparison_sampler,
            suv,
            layer,
            sdepth + bias,
        );
    }
    return lit / 12.0;
}
