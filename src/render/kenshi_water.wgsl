// Kenshi's water, ported from data/materials/forward/water.hlsl (waterVP / waterFP; the zone
// water's BLEND variant and the distant plane's DISTANT one).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view
#import bevy_pbr::view_transformations::{depth_ndc_to_view_z, position_world_to_view}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif
#import kenshi::lighting::{punctual, lazarov, fresnel_diffuse, reflection_dir, reflection_mip,
    apply_fog, to_screen, Light, PI}

struct WaterParams {
    // the vertex shader's scale: water (xy), scum (zw)
    scale: vec4<f32>,
    // invStrength, invOpacity, gloss, glow
    consts: vec4<f32>,
    // xy: distortion; z: blendChannel (0..4, -1: not blended); w: 1 for the distant plane
    distortion: vec4<f32>,
    sun_direction: vec4<f32>,
    sun_colour: vec4<f32>,
    env_colour: vec4<f32>,
    sun_real: vec4<f32>,
    horizon: vec4<f32>,
    fog: vec4<f32>,
    fog_colour: vec4<f32>,
    // xyz: the scene origin in world units; w: gameTime
    origin_time: vec4<f32>,
    // x: the HDR composite's exposure; y: rainAmount; z: debug view (0 none)
    world: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: WaterParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var colour_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var map_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var flow_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var turbulence_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var normal_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var scum_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var scum_normal: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var blend_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var irradiance_cube: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var specularity_cube: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var cube_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var rain_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var reflection_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(14) var reflection_sampler: sampler;

fn rain_ripple(coord: vec2<f32>, time: f32, weight: f32) -> vec3<f32> {
    let ripple = textureSample(rain_map, map_sampler, coord);
    let drop_time = fract(ripple.a + time * 0.5);
    let drop_frac = drop_time - 1.0 + ripple.r * 0.5 + 0.5;
    let s = -sin(clamp(drop_frac * 16.0, 0.0, 2.0) * 3.14159);
    var n = ripple.grb;
    n = vec3<f32>(n.x * s, n.y, n.z * s);
    return n * weight * ripple.r;
}

const MAP_MIN: f32 = -147456.0;
const MAP_SIZE: f32 = 294912.0;

fn sample_normal(coord: vec2<f32>, direction: vec2<f32>, speed: f32, time: f32) -> vec3<f32> {
    let t = fract(time);
    var n = textureSample(normal_map, map_sampler, coord + direction * speed * t).rgb;
    n = n * 2.0 - 1.0;
    let weight = 1.0 - abs(t * 2.0 - 1.0);
    return n.xzy * weight;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let distant = p.distortion.w > 0.5;
    let origin = p.origin_time.xyz;
    let world_pos = in.world_position.xyz + origin;
    let camera = view.world_position + origin;
    let cam = camera - world_pos;

    // waterVP
    let base = world_pos.xz;
    var tex = vec4<f32>(base * p.scale.xy, base * p.scale.zw);
    let speed_mult = p.scale.xy * 5000.0;
    let map_xy = (base - vec2<f32>(MAP_MIN)) / MAP_SIZE;

    // waterFP
    let dist = length(cam);
    let view_dir = normalize(cam);
    let light_dir = normalize(p.sun_direction.xyz);
    let light_colour = p.sun_colour.rgb;
    let spec_colour = vec3<f32>(0.04);
    let water_colour = textureSample(colour_map, map_sampler, map_xy).rgb;
    var albedo = water_colour;
    var gloss = p.consts.z;

    if (distant) {
        if (dist - 4000.0 < 0.0 || map_xy.x < 0.000001 || map_xy.y < 0.000001
            || map_xy.x > 0.99999 || map_xy.y > 0.99999) {
            discard;
        }
    }

    // flow
    var info = textureSample(flow_map, map_sampler, map_xy);
    var direction = info.rg * 2.0 - 1.0;
    let turbulence = textureSample(turbulence_map, map_sampler, tex.xy * 0.234).xy * 2.0 - 1.0;
    if (!distant) {
        direction = direction + turbulence * 0.6;
    }
    direction = direction * speed_mult;
    var speed = length(direction);
    direction = direction / speed;
    speed = speed / p.distortion.x;
    let game_time = p.origin_time.w * p.distortion.x;

    // depth: the scene behind the water, along the view ray
    var depth = 1e6;
#ifdef DEPTH_PREPASS
    if (!distant) {
        let ndc = prepass_depth(in.position, 0u);
        if (ndc > 0.0) {
            let ground_z = depth_ndc_to_view_z(ndc);
            let water_z = position_world_to_view(in.world_position.xyz).z;
            depth = max(0.0, dist * (ground_z / water_z) - dist);
        }
    }
#endif

    // the normal
    var normal = vec3<f32>(0.0, 1.0, 0.0);
    normal = normal + sample_normal(tex.xy, direction, speed, game_time);
    normal = normal + sample_normal(tex.xy + vec2<f32>(0.1, 0.3), direction, speed, game_time + 0.33);
    normal = normal + sample_normal(tex.xy + vec2<f32>(0.4, 0.7), direction, speed, game_time + 0.66);
    normal.y = normal.y * p.consts.x;
    // rain ripples (rainAmount: the weather's rain / 50)
    let map_zw = base * 0.01;
    let ripples_a = rain_ripple(map_zw * 6.0, game_time * 1.5, 1.0);
    let ripples_b = rain_ripple(map_zw.yx * 6.0, game_time * 1.5 + 0.5, 1.0);
    let ripples_c = rain_ripple(map_zw * 2.0, game_time + 0.3, 1.0);
    if (!distant) {
        let rain = p.world.y * clamp(view_dir.y * 2.0 - dist * 0.0001, 0.0, 1.0);
        normal = normal + ripples_a * rain * 2.0 + ripples_b * rain * 2.0 + ripples_c * rain * 0.8;
    }
    normal = normalize(normal);

    // the reflection (kenshi_render::reflection): rt_reflection's red at the fragment's screen
    // place (refTex: the clip position x bias, divided by w), moved by the normal (x 60) and down
    // with the distance; read flipped left to right (the harness's unmirrored camera)
    let clip = view.clip_from_world * vec4<f32>(in.world_position.xyz, 1.0);
    var ref_tex = vec4<f32>(clip.x * 0.5 + clip.w * 0.5, -clip.y * 0.5 + clip.w * 0.5, 0.0, clip.w);
    ref_tex = vec4<f32>(ref_tex.xy + normal.xz * 60.0, ref_tex.zw);
    ref_tex.y = ref_tex.y + 12.0 * clamp(dist * 0.005, 0.0, 1.0);
    let ref_uv = ref_tex.xy / ref_tex.w;
    let rf = vec3<f32>(textureSample(reflection_map, reflection_sampler, vec2<f32>(1.0 - ref_uv.x, ref_uv.y)).r);

    // scum
    tex = vec4<f32>(tex.xy, tex.zw + normal.xz * p.distortion.y);
    let scum_albedo = textureSample(scum_map, map_sampler, tex.zw);
    let scum_n = textureSample(scum_normal, map_sampler, tex.zw);
    if (!distant) {
        info.b = info.b * scum_n.a;
        gloss = mix(gloss, scum_albedo.a, clamp(info.b * 4.0, 0.0, 1.0));
        albedo = mix(albedo, scum_albedo.rgb, info.b);
        normal = normalize(mix(normal, scum_n.xzy, info.b));
    } else {
        info.b = 0.0;
    }

    // lighting (CalcPunctualLight, CalcEnvironmentLight)
    let sun_light = punctual(normal, light_dir, view_dir, gloss, spec_colour, light_colour, 0.0);
    let nov = clamp(dot(view_dir, normal), 0.0, 1.0);
    let irr = textureSampleLevel(irradiance_cube, cube_sampler, normal, 3.0);
    let refl = textureSampleLevel(specularity_cube, cube_sampler,
        reflection_dir(normal, view_dir, gloss), reflection_mip(gloss));
    var env: Light;
    env.specular = lazarov(gloss, nov, spec_colour) * (refl.rgb * refl.a * 10.0);
    env.diffuse = (irr.rgb * irr.a * 4.0) * fresnel_diffuse(spec_colour);
    let env_w = p.env_colour.w * clamp(light_dir.y * 5.0 + 0.2, 0.1, 1.0);
    env.diffuse = env.diffuse * p.env_colour.rgb * env_w;
    env.specular = env.specular * p.env_colour.rgb * env_w;
    env.specular = env.specular * rf * env_w;
    var colour = sun_light.specular + env.specular + (sun_light.diffuse + env.diffuse) * albedo;
    colour = colour + p.consts.w * water_colour * (1.0 - info.b);

    var alpha = 1.0;
    if (!distant) {
        var fresnel = dot(view_dir, normal);
        fresnel = 1.0 - pow(1.0 - fresnel, 2.0);
        fresnel = fresnel * (1.0 - info.b);
        alpha = clamp(1.0 - (dist - 4000.0) / 1000.0, 0.0, 1.0);
        alpha = alpha * mix(1.0, clamp(depth * p.consts.y, 0.0, 1.0), fresnel);
        alpha = alpha * clamp(depth / 2.0, 0.0, 1.0);
        alpha = mix(1.0, alpha, clamp((4000.0 - dist) / 400.0, 0.0, 1.0));
        let channel = i32(p.distortion.z);
        if (channel >= 0) {
            let biome = textureSample(blend_map, map_sampler, map_xy);
            var blend: f32;
            if (channel < 4) {
                blend = biome[channel];
            } else {
                blend = 1.0 - dot(biome, vec4<f32>(1.0));
            }
            alpha = alpha * blend;
        }
    }
    // debug views (water.hlsl's MODE, testing): 1 alpha, 2 depth / 500, 3 scum (info.b), 4 albedo; 5 the reflection (rf)
    let mode = i32(p.world.z);
    if (mode == 1) { return vec4<f32>(vec3<f32>(alpha), 1.0); }
    if (mode == 2) { return vec4<f32>(vec3<f32>(clamp(depth / 500.0, 0.0, 1.0)), 1.0); }
    if (mode == 3) { return vec4<f32>(vec3<f32>(info.b), 1.0); }
    if (mode == 5) { return vec4<f32>(rf, 1.0); }
    if (mode == 4) { return vec4<f32>(albedo, 1.0); }
    // (assumption) the atmosphere fog over the water at its own distance
    colour = apply_fog(colour, camera, world_pos, p.sun_real.xyz, p.horizon, p.fog, p.fog_colour,
        p.sun_colour.w);
    return vec4<f32>(to_screen(colour, p.world.x), alpha);
}
