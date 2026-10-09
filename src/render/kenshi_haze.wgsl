// Kenshi's heat haze (data/materials/post/heathaze.hlsl; the HeatHaze option): the image
// shimmered by a flowing perturbation, more with depth, by the weather's heatHaze. It is the last
// post effect (compositors.cfg: FXAA, then HeatHaze), on the 8-bit display image; the image is
// read clamped and bilinear, the flow and perturbation maps wrapped with Kenshi's default
// filtering (anisotropic x16).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct HazeParams {
    // x: gameTime; y: heatHaze; z: near clip; w: far clip
    a: vec4<f32>,
    // xy: tan of half the field of view across and up
    b: vec4<f32>,
}

@group(0) @binding(0) var image_map: texture_2d<f32>;
@group(0) @binding(1) var lin: sampler;
@group(0) @binding(2) var flow_map: texture_2d<f32>;
@group(0) @binding(3) var perturbation: texture_2d<f32>;
@group(0) @binding(4) var depth_map: texture_depth_2d;
@group(0) @binding(5) var<uniform> p: HazeParams;
@group(0) @binding(6) var wrap: sampler;

fn sample_normal(coord: vec2<f32>, direction: vec2<f32>, speed: f32, time: f32) -> vec3<f32> {
    let t = fract(time);
    var n = textureSample(perturbation, wrap, coord + direction * speed * t).rgb;
    n = n * 2.0 - 1.0;
    let weight = 1.0 - abs(t * 2.0 - 1.0);
    return n.xzy * weight;
}

@fragment
fn haze(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let tex = in.uv * 7.341;
    let map = in.uv * 3.341;
    let speed = 1.0;
    let game_time = p.a.x * 100.0;
    let info = textureSample(flow_map, wrap, map);
    let direction = info.rg * 2.0 - 1.0;
    var normal = vec3<f32>(0.0, 1.0, 0.0);
    normal += sample_normal(tex, direction, speed, game_time);
    normal += sample_normal(tex + vec2<f32>(0.1, 0.3), direction, speed, game_time + 0.33);
    normal += sample_normal(tex + vec2<f32>(0.4, 0.7), direction, speed, game_time + 0.66);
    normal.y = 0.0;
    normal = normalize(normal);
    // the G-buffer's depth: the distance to the pixel over the far clip (none, the sky: 1);
    // from the reverse infinite depth, view z = near / ndc, along the pixel's ray
    let dims = vec2<f32>(textureDimensions(depth_map));
    let texel = min(vec2<i32>(in.uv * dims), vec2<i32>(dims) - 1);
    let ndc = textureLoad(depth_map, texel, 0);
    var depth = 1.0;
    if (ndc > 0.0) {
        let ray = vec3<f32>((in.uv.x * 2.0 - 1.0) * p.b.x, (1.0 - in.uv.y * 2.0) * p.b.y, 1.0);
        depth = clamp(p.a.z / ndc * length(ray) / p.a.w, 0.0, 1.0);
    }
    normal = normal * clamp(depth * 6.0, 0.0, 1.0) * 0.002 * p.a.y;
    var fragment = textureSample(image_map, lin, in.uv + normal.xz);
    fragment = mix(fragment, textureSample(image_map, lin, in.uv + normal.xz * 0.7), 0.5);
    return fragment;
}
