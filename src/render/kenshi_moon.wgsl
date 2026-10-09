// Kenshi's moons (data/materials/forward/moon.hlsl): the planet's texture lit by the sun, with the
// atmosphere's scattering toward it added in front. The vertex shader's scattering runs here per
// pixel; the moon sits at the far plane (moon.hlsl forces z to the far clip).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view
#import kenshi::lighting::{sky_scale, SKY_INNER, SKY_OUTER, SKY_HEIGHT, SKY_KR, SKY_KM, SKY_ESUN,
    SKY_EXPOSURE, PI}

struct MoonParams {
    // xyz: sunDirectionReal
    sun_real: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> p: MoonParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var tex_sampler: sampler;

struct MoonOut {
    @location(0) colour: vec4<f32>,
    // the far plane (reverse depth: 0), as moon.hlsl forces
    @builtin(frag_depth) depth: f32,
}

@fragment
fn fragment(in: VertexOutput) -> MoonOut {
    let light_dir = p.sun_real.xyz;
    // main_vs: the scattering toward the moon
    let inv_wl = 1.0 / pow(vec3<f32>(0.57, 0.48, 0.44), vec3<f32>(4.0));
    let kr_esun = SKY_KR * SKY_ESUN;
    let kr4pi = SKY_KR * 4.0 * PI;
    let km4pi = SKY_KM * 4.0 * PI;
    let scale = 1.0 / (SKY_OUTER - SKY_INNER);
    let scale_depth = (SKY_OUTER - SKY_INNER) * 0.5;
    let scale_over_depth = scale / scale_depth;
    let cam = vec3<f32>(0.0, SKY_INNER + (SKY_OUTER - SKY_INNER) * SKY_HEIGHT, 0.0);
    var pos = normalize(in.world_position.xyz - view.world_position);
    pos.y = pos.y + SKY_INNER;
    var ray = pos - cam;
    let far = length(ray);
    ray = ray / far;
    let start_angle = dot(ray, cam) / cam.y;
    let depth0 = exp(scale_over_depth * (SKY_INNER - cam.y));
    let start_offset = depth0 * sky_scale(start_angle, scale_depth);
    let sample_length = far / 4.0;
    let scaled_length = sample_length * scale;
    let sample_ray = ray * sample_length;
    var point = cam + sample_ray * 0.5;
    var colour = vec3<f32>(0.0);
    for (var i = 0; i < 4; i = i + 1) {
        let height = length(point);
        let depth = exp(scale_over_depth * (SKY_INNER - height));
        let light_angle = dot(light_dir, point) / height;
        let camera_angle = dot(ray, point) / height;
        let scatter = start_offset + depth * (sky_scale(light_angle, scale_depth) - sky_scale(camera_angle, scale_depth));
        colour = colour + exp(-scatter * (inv_wl * kr4pi + km4pi)) * (depth * scaled_length);
        point = point + sample_ray;
    }
    let rayleigh = colour * (inv_wl * kr_esun);
    let coss = dot(light_dir, ray);
    let phase = 0.75 * (1.0 + 0.5 * coss * coss);
    let air = phase * rayleigh * SKY_EXPOSURE;
    // main_fs
    var result = textureSample(tex, tex_sampler, in.uv);
    result = vec4<f32>(result.rgb * clamp(dot(normalize(in.world_normal), light_dir), 0.0, 1.0) + air, 1.0);
    var out: MoonOut;
    out.colour = result;
    out.depth = 0.0;
    return out;
}
