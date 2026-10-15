// CLIP_INTERIOR (terrainfp4.hlsl, mapfeature.hlsl, foliage.hlsl): the interior mask
// (interior_mask.rs) clips a fragment nearer than its volume's far side and farther than its
// near side:
//     float2 interior = interiorMask.Sample(Nearest, fragCoord.xy * viewport.zw);
//     if(fragCoord.w < interior.y) clip(interior.x - fragCoord.w);
// A near side behind the far side is none (the camera inside the volume: Kenshi's depth test
// drew none). Only in a view of the mask's size (the main camera's, not the water reflection's)
// and not in an orthographic one (the sun's shadows).
#define_import_path kenshi::interior

#import bevy_pbr::mesh_view_bindings::view

fn interior_clipped(mask: texture_2d<f32>, frag: vec4<f32>, world_position: vec3<f32>) -> bool {
    let dims = vec2<f32>(textureDimensions(mask));
    if (any(abs(dims - view.viewport.zw) > vec2<f32>(0.5)) || view.clip_from_view[3][3] == 1.0) {
        return false;
    }
    // the pixel and view depth from the world position itself (the same in the prepass and the
    // main pass)
    let clip = view.clip_from_world * vec4<f32>(world_position, 1.0);
    let ndc = clip.xy / clip.w;
    let pixel = vec2<i32>((ndc * vec2<f32>(0.5, -0.5) + 0.5) * view.viewport.zw);
    let m = textureLoad(mask, clamp(pixel, vec2<i32>(0), vec2<i32>(dims) - 1), 0).rg;
    let w = clip.w;
    let near = select(m.x, 0.0, m.x > m.y);
    return w < m.y && w > near;
}
