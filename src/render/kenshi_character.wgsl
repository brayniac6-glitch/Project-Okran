// Kenshi's characters: data/materials/deferred/character.hlsl main_fs (skin: body and head),
// skin.hlsl main_fs (skinned clothing) and character.hlsl hair_fs, lit by the deferred main light
// (kenshi::lighting) in the same pass.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view
#import kenshi::lighting::{apply_fog, local_lights, main_light, make_wet, reflection_dir, reflection_mip,
    sun_shadow, to_screen}

struct CharacterParams {
    // x: kind (0 skin, 1 clothing, 2 hair); y: glossMult; z: alpha threshold; w: water height
    kind: vec4<f32>,
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
    // the head's hair and beard maps (character.hlsl hairMult, hairAlpha, beardAlpha, hairColor)
    hair_mult: vec4<f32>,
    hair_alpha: vec4<f32>,
    beard_alpha: vec4<f32>,
    hair_colour: vec4<f32>,
    // x: 1 with painted-on clothing (the vest maps); y: 1 painted clothing (the colour map)
    vest: vec4<f32>,
    // skin.hlsl's color1, color2
    paint1: vec4<f32>,
    paint2: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: CharacterParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var normal_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var ambient_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var irradiance_cube: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var specularity_cube: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var cube_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var hair_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var beard_map: texture_2d<f32>;
// hairMap / beardMap (border, anisotropic) and the ambient map (clamped)
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var scalp_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var ambient_sampler: sampler;
// common/shadowFunctions.hlsl jitterMap (white-noise.png)
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var shadow_jitter: texture_2d<f32>;
// painted-on clothing (a shirt's vest texture and normal map)
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var vest_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(14) var vest_normal_map: texture_2d<f32>;
// skin.hlsl's colorMap (colourisation in red and green, metalness in blue)
@group(#{MATERIAL_BIND_GROUP}) @binding(15) var color_map: texture_2d<f32>;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2<f32>(0.0);
#endif
    let kind = i32(p.kind.x);
    // Skinned's diffuseMap: mipmap_bias -2
    var diffuse = textureSampleBias(diffuse_map, diffuse_sampler, uv, -2.0);
    let normal_tex = textureSample(normal_map, diffuse_sampler, uv);
    // painted-on clothing over the skin by its normal map's alpha (Kenshi's vest)
    if (p.vest.x > 0.5) {
        let vest = textureSampleBias(vest_map, diffuse_sampler, uv, -2.0);
        let vest_n = textureSample(vest_normal_map, diffuse_sampler, uv);
        diffuse = vec4<f32>(mix(diffuse.rgb, vest.rgb, vest_n.a), diffuse.a);
    }
    let origin = p.origin_exposure.xyz;
    let world_pos = in.world_position.xyz + origin;

    let n = normalize(in.world_normal);
#ifdef VERTEX_TANGENTS
    let t = normalize(in.world_tangent.xyz);
    let b = normalize(cross(in.world_normal, in.world_tangent.xyz) * in.world_tangent.w);
#else
    let t = vec3<f32>(1.0, 0.0, 0.0);
    let b = vec3<f32>(0.0, 0.0, 1.0);
#endif
    var world_normal = n;
    var gloss = 0.0;
    var metalness = 0.0;
    var albedo = diffuse.rgb;
    if (kind == 0) {
        // character.hlsl: DXT5 normals (x in alpha), gloss in the diffuse alpha
        var nxy = vec2<f32>(normal_tex.a, normal_tex.g) * 2.0 - 1.0;
        let nz = sqrt(max(0.0, 1.0 - dot(nxy, nxy)));
        world_normal = normalize(nxy.x * t + nxy.y * b + nz * n);
        // hair and beard drawn on the head
        let hair = textureSample(hair_map, scalp_sampler, uv);
        let beard = textureSample(beard_map, scalp_sampler, uv);
        let hair_value = clamp(dot(hair, p.hair_alpha), 0.0, 1.0);
        let beard_value = clamp(dot(beard, p.beard_alpha), 0.0, 1.0);
        let hair_col = p.hair_colour.rgb * clamp(dot(hair, p.hair_mult), 0.0, 1.0);
        let beard_col = p.hair_colour.rgb * beard_value;
        diffuse = vec4<f32>(mix(diffuse.rgb, hair_col, hair_value), diffuse.a);
        diffuse = vec4<f32>(mix(diffuse.rgb, beard_col, beard_value), diffuse.a);
        gloss = diffuse.a;
        // makeWet: dry (wet.x 0) unless under the water
        let absorb = (1.0 - gloss * 2.0) * (0.8 * 0.0 + 0.2);
        let w = make_wet(vec4<f32>(diffuse.rgb, gloss), 0.0, absorb, p.kind.w - world_pos.y, 0.5);
        albedo = w.rgb * 0.7;
        // stop texture hair being super shiny
        gloss = w.a * mix(1.0, 0.4, max(hair_value, beard_value));
    } else if (kind == 1) {
        // skin.hlsl: cut where the normal map's alpha is under 0.6, green flipped
        if (normal_tex.a - 0.6 < 0.0) {
            discard;
        }
        // the colour map: colourisation data and metalness (COLOURING)
        let cmap = textureSample(color_map, diffuse_sampler, uv).rgb;
        metalness = cmap.b;
        if (p.vest.y > 0.5) {
            let intensity = (diffuse.r + diffuse.g + diffuse.b) / 3.0;
            let m1 = mix(intensity, 1.0, p.paint1.a);
            let m2 = mix(intensity, 1.0, p.paint2.a);
            diffuse = vec4<f32>(mix(diffuse.rgb, p.paint1.rgb * m1, cmap.r), diffuse.a);
            diffuse = vec4<f32>(mix(diffuse.rgb, p.paint2.rgb * m2, cmap.g), diffuse.a);
        }
        let bump = vec3<f32>(normal_tex.r, 1.0 - normal_tex.g, normal_tex.b) * 2.0 - 1.0;
        world_normal = normalize(bump.x * t + bump.y * b + bump.z * n);
        diffuse.a = diffuse.a * p.kind.y;
        let absorb = 1.0 - diffuse.a * 2.0;
        let w = make_wet(diffuse, 0.0, absorb, p.kind.w - world_pos.y, 0.5);
        albedo = w.rgb * 0.8;
        gloss = w.a;
    } else if (kind == 3) {
        // creature.hlsl (Creature_FP): DXT5 normals (x in alpha) with green flipped, gloss in the
        // diffuse alpha, the diffuse as it is
        var nxy = vec2<f32>(normal_tex.a, normal_tex.g) * 2.0 - 1.0;
        let nz = sqrt(max(0.0, 1.0 - dot(nxy, nxy)));
        nxy.y = -nxy.y;
        world_normal = normalize(nxy.x * t + nxy.y * b + nz * n);
        let absorb = 1.0 - diffuse.a * 2.0;
        let w = make_wet(diffuse, 0.0, absorb, p.kind.w - world_pos.y, 0.5);
        albedo = w.rgb;
        gloss = w.a;
    } else {
        // hair_fs (diffuseChannel in hair_mult, alphaChannel in hair_alpha, color and
        // specularMult in hair_colour, threshold in beard_alpha.x)
        let d = diffuse * p.hair_mult;
        let a = diffuse * p.hair_alpha;
        let brightness = d.r + d.g + d.b + d.a;
        let alpha = a.r + a.g + a.b + a.a;
        if (alpha - p.beard_alpha.x < 0.0) {
            discard;
        }
        albedo = brightness * p.hair_colour.rgb * 0.7;
        gloss = brightness * p.hair_colour.w;
        if (!front) {
            world_normal = -world_normal;
        }
    }

    let camera = view.world_position + origin;
    // the water reflection's view (kenshi_object.wgsl): the albedo's luma
    if (camera.y < 100.0) {
        if (world_pos.y < 100.0) {
            discard;
        }
        return vec4<f32>(0.25 * albedo.r + 0.5 * albedo.g + 0.25 * albedo.b, 0.0, 0.0, 1.0);
    }
    let view_dir = normalize(camera - world_pos);
    let world_size = p.world.x;
    let ambient_mult = textureSample(ambient_map, ambient_sampler, (world_pos.xz + world_size * 0.5) / world_size);
    // (the G-buffer's 8-bit gloss: clamped to 0..1)
    gloss = clamp(gloss, 0.0, 1.0);
    let irr = textureSampleLevel(irradiance_cube, cube_sampler, world_normal, 3.0);
    let refl = textureSampleLevel(specularity_cube, cube_sampler,
        reflection_dir(world_normal, view_dir, gloss), reflection_mip(gloss));
    let lit = main_light(world_normal, view_dir, albedo, gloss, metalness, 0.0, 0.0,
        p.sun_direction.xyz, p.sun_colour, p.env_colour.w, p.ambient_params.w,
        ambient_mult, irr, refl, sun_shadow(in.world_position, n, shadow_jitter));
    let lit_all = lit + local_lights(in.world_position.xyz, in.position.xy, world_normal, view_dir, albedo, gloss, metalness);
    let fogged = apply_fog(lit_all, camera, world_pos, p.sun_real.xyz, p.horizon, p.fog, p.fog_colour,
        p.sun_colour.w);
    return vec4<f32>(to_screen(fogged, p.origin_exposure.w), 1.0);
}
