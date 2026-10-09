// Kenshi's HDR chain (data/materials/post/hdrfp4.hlsl, hdrutils.hlsl; compositor main.compositor):
// the scene's luminance downsampled 2x2 to 128 x 128, then 3x3 to 64, 16, 4 and 1; adapted toward
// it over time (Pattanaik), clamped to MIN_LUMINANCE .. MAX_LUMINANCE and kept as its log; the
// scene composited at EXPOSURE_KEY / the adapted luminance (no tone curve, no bloom: BLOOM_MAGNITUDE
// 0), into an 8-bit target as Kenshi's (the post effects' PF_A8R8G8B8 buffers). Kenshi shows the
// result as it is (sRGB conversion off), so `present` hands it to the sRGB target decoded.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct HdrParams {
    // frameTime, MIN_LUMINANCE, MAX_LUMINANCE, AUTOEXP_ADAPTATION_RATE
    a: vec4<f32>,
    // EXPOSURE_KEY
    b: vec4<f32>,
}

@group(0) @binding(0) var in_rtt: texture_2d<f32>;
@group(0) @binding(1) var lin: sampler;
@group(0) @binding(2) var in_lum: texture_2d<f32>;
@group(0) @binding(3) var<uniform> p: HdrParams;

fn calc_luminance(c: vec3<f32>) -> f32 {
    return max(dot(c, vec3<f32>(0.299, 0.587, 0.114)), 0.0001);
}

fn texel() -> vec2<f32> {
    return 1.0 / vec2<f32>(textureDimensions(in_rtt));
}

fn adapted() -> f32 {
    return exp(textureLoad(in_lum, vec2<i32>(0, 0), 0).r);
}

@fragment
fn downsample_2x2_luminance(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let t = texel();
    var v = 0.0;
    v += calc_luminance(textureSample(in_rtt, lin, in.uv + t * vec2<f32>(-0.5, -0.5)).rgb);
    v += calc_luminance(textureSample(in_rtt, lin, in.uv + t * vec2<f32>(-0.5, 0.5)).rgb);
    v += calc_luminance(textureSample(in_rtt, lin, in.uv + t * vec2<f32>(0.5, -0.5)).rgb);
    v += calc_luminance(textureSample(in_rtt, lin, in.uv + t * vec2<f32>(0.5, 0.5)).rgb);
    return vec4<f32>(v / 4.0, 0.0, 0.0, 1.0);
}

@fragment
fn downsample_3x3(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let t = texel();
    var v = vec4<f32>(0.0);
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            v += textureSample(in_rtt, lin, in.uv + t * vec2<f32>(f32(x), f32(y)));
        }
    }
    return v / 9.0;
}

@fragment
fn adapt_luminance(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let last = adapted();
    let current = textureSample(in_rtt, lin, vec2<f32>(0.5, 0.5)).r;
    var lum = last + (current - last) * (1.0 - exp(-p.a.x * p.a.w));
    lum = clamp(lum, p.a.y, p.a.z);
    return vec4<f32>(log(lum), 0.0, 0.0, 1.0);
}

@fragment
fn composite(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(in_rtt, lin, in.uv).rgb;
    // CalcExposedColor: exp2(log2(max(key / lum, 0.001)))
    let exposure = max(p.b.x / adapted(), 0.001);
    return vec4<f32>(clamp(c * exposure, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

@fragment
fn present(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let x = textureLoad(in_rtt, vec2<i32>(in.position.xy), 0).rgb;
    let lo = x / 12.92;
    let hi = pow((x + 0.055) / 1.055, vec3<f32>(2.4));
    return vec4<f32>(select(hi, lo, x <= vec3<f32>(0.04045)), 1.0);
}
