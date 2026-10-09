// SkyX's skydome (data/materials/SkyX/SkyX_Skydome.hlsl, HDR with STARFIELD), drawn on a sphere
// round the camera. Its vertex shader's scattering runs here per pixel, along the pixel's
// direction from the dome's centre; the result goes through the HDR composite's exposure.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view
#import kenshi::lighting::{sky_scale, to_screen, SKY_INNER, SKY_OUTER, SKY_HEIGHT, SKY_KR, SKY_KM,
    SKY_ESUN, SKY_EXPOSURE, PI}

struct SkyParams {
    // xyz: toward the sun (uLightDir); w: uTime
    light: vec4<f32>,
    // x: the HDR composite's exposure
    exposure: vec4<f32>,
    // the cloud layer (SkyX_Clouds.hlsl): zenithLight, sunColour, xy uWindDirection
    zenith: vec4<f32>,
    sun_colour: vec4<f32>,
    wind: vec4<f32>,
    // Height, Scale, DistanceAttenuation, DetailAttenuation
    cloud_a: vec4<f32>,
    // HeightVolume, VolumetricDisplacement, DensityMultiplier, DensityOffset
    cloud_b: vec4<f32>,
    // Darkness
    cloud_c: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: SkyParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var starfield: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var star_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var clouds: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var clouds_normal: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var clouds_tile: texture_2d<f32>;

// SkyX_Clouds.hlsl main_fp (HDR): the cloud layer's colour and alpha toward `pos` (the dome's
// normalised position).
fn cloud_layer(pos: vec3<f32>) -> vec4<f32> {
    let height = p.cloud_a.x;
    let scale = p.cloud_a.y;
    let distance_attenuation = p.cloud_a.z;
    let height_volume = p.cloud_b.x;
    let volumetric = p.cloud_b.y;
    let density_mult = p.cloud_b.z;
    let density_offset = p.cloud_b.w;
    let darkness = p.cloud_c.x;
    var vh = height / pos.y;
    var cloud_pos = pos * vh;
    let wind = p.wind.xy * 0.00005;
    var uv = cloud_pos.xz * scale;
    var density = textureSampleLevel(clouds, star_sampler, uv + wind, 0.0).r;
    var normal = -(2.0 * textureSampleLevel(clouds_normal, star_sampler, uv + wind, 0.0).rgb - 1.0);
    normal = vec3<f32>(normal.x, normal.z, normal.y);
    density = clamp((density + density_offset) * density_mult, 0.0, 1.0);
    // volumetric effect
    let layer_height_volume = height_volume * pos.y;
    let layer_displacement = volumetric * pos.y;
    let new_pos = normalize(pos + layer_displacement * vec3<f32>(normal.x, 0.0, normal.z));
    vh = (height + height * (1.0 - density) * layer_height_volume) / new_pos.y;
    cloud_pos = new_pos * vh;
    uv = cloud_pos.xz * scale;
    density = textureSampleLevel(clouds, star_sampler, uv + wind + vec2<f32>(0.2, 0.6), 0.0).r;
    density = (density + density_offset) * density_mult;
    let tile = textureSampleLevel(clouds_tile, star_sampler, uv - wind, 0.0).r;
    density = density + tile * 0.1;
    var colour = p.zenith.rgb + p.sun_colour.rgb * (1.0 - density * 0.1);
    let distance = clamp(10.0 * clamp(-distance_attenuation + pos.y, 0.0, 1.0), 0.0, 1.0);
    density = density + distance;
    colour = colour * (1.0 - clamp(density, 0.0, 1.0) * darkness);
    var alpha = density * clamp(1.0 - tile + density_offset, 0.0, 1.0);
    alpha = mix(density_offset + 0.5, alpha, distance);
    return vec4<f32>(clamp(colour, vec3<f32>(0.0), vec3<f32>(1.0)) * pow(SKY_EXPOSURE, 0.5),
        clamp(alpha, 0.0, 1.0));
}

const SKY_SAMPLES: i32 = 4;
const SKY_G: f32 = -0.991;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_position.xyz - view.world_position);
    var n = dir;
    // (assumption) below the horizon the dome's lower rings keep the horizon's colour
    n.y = max(n.y, 0.0);
    let light_dir = normalize(p.light.xyz);

    // main_vp
    let inv_wl = 1.0 / pow(vec3<f32>(0.57, 0.48, 0.44), vec3<f32>(4.0));
    let kr_esun = SKY_KR * SKY_ESUN;
    let km_esun = SKY_KM * SKY_ESUN;
    let kr4pi = SKY_KR * 4.0 * PI;
    let km4pi = SKY_KM * 4.0 * PI;
    let scale = 1.0 / (SKY_OUTER - SKY_INNER);
    let scale_depth = (SKY_OUTER - SKY_INNER) * 0.5;
    let scale_over_depth = scale / scale_depth;
    let cam = vec3<f32>(0.0, SKY_INNER + (SKY_OUTER - SKY_INNER) * SKY_HEIGHT, 0.0);

    var pos = n;
    pos.y = pos.y + SKY_INNER;
    var ray = pos - cam;
    let far = length(ray);
    ray = ray / far;
    let start_angle = dot(ray, cam) / cam.y;
    let depth0 = exp(scale_over_depth * (SKY_INNER - cam.y));
    let start_offset = depth0 * sky_scale(start_angle, scale_depth);
    let sample_length = far / f32(SKY_SAMPLES);
    let scaled_length = sample_length * scale;
    let sample_ray = ray * sample_length;
    var point = cam + sample_ray * 0.5;
    var thickness = 0.0;
    var colour = vec3<f32>(0.0);
    for (var i = 0; i < SKY_SAMPLES; i = i + 1) {
        let height = length(point);
        let depth = exp(scale_over_depth * (SKY_INNER - height));
        let light_angle = dot(light_dir, point) / height;
        let camera_angle = dot(ray, point) / height;
        let scatter = start_offset + depth * (sky_scale(light_angle, scale_depth) - sky_scale(camera_angle, scale_depth));
        colour = colour + exp(-scatter * (inv_wl * kr4pi + km4pi)) * (depth * scaled_length);
        thickness = thickness + depth * scaled_length;
        point = point + sample_ray;
    }
    let rayleigh_colour = colour * (inv_wl * kr_esun);
    let mie_colour = colour * km_esun;
    let direction = cam - pos;
    // the dome's UVs (SkyX MeshManager::_updateGeometry, as recalled from SkyX's source, not
    // checked against SkyX_x64.dll): (1 + (cos, sin) of the bearing x the angle from the zenith
    // over 90 degrees) x 4, 1 below the horizon; the vertex shader takes a tenth
    let rim = length(n.xz);
    let around = select(vec2<f32>(0.0), n.xz / rim, rim > 1e-5);
    let uvr = min(acos(clamp(n.y, -1.0, 1.0)) / (PI * 0.5), 1.0);
    let uv = (1.0 + around * uvr) * 4.0 * 0.1;
    let opacity_y = thickness * kr4pi;
    let height_v = 1.0 - n.y;

    // main_fp, HDR
    let c = dot(light_dir, direction) / length(direction);
    let c2 = c * c;
    let rayleigh_phase = 0.75 * (1.0 + 0.5 * c2);
    let g2 = SKY_G * SKY_G;
    let mie_phase = 1.5 * ((1.0 - g2) / (2.0 + g2)) * (1.0 + c2) / pow(1.0 + g2 - 2.0 * SKY_G * c, 1.5);
    var out = SKY_EXPOSURE * (rayleigh_phase * rayleigh_colour + mie_phase * mie_colour);
    let night = clamp(1.0 - max(out.x, max(out.y, out.z)) * 10.0, 0.0, 1.0)
        * (1.0 - clamp(opacity_y, 0.0, 1.0));
    let below = clamp(-light_dir.y, 0.0, 1.0);
    let glow = pow(vec3<f32>(0.05, 0.05, 0.1) * (2.0 - 0.75 * below) * pow(height_v, 3.0), vec3<f32>(2.2));
    let stars = textureSample(starfield, star_sampler, uv + p.light.w * 0.1).rgb
        * (0.35 + clamp(-light_dir.y * 0.45, 0.0, 1.0)) * 2.0;
    out = out + night * (glow + stars);
    // the cloud layer, alpha blended over the sky (above the horizon: the dome's lower rings
    // carry no clouds)
    if (dir.y > 0.0) {
        let c = cloud_layer(dir);
        out = mix(out, c.rgb, c.a);
    }
    return vec4<f32>(to_screen(out, p.exposure.x), 1.0);
}
