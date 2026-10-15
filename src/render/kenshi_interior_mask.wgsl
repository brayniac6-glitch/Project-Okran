// interiormask.hlsl: each face writes its view depth + 1 (pos.w + 1); the material's scheme
// picks the channel (see interior_mask.rs).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let w = -(view.view_from_world * vec4<f32>(in.world_position.xyz, 1.0)).z + 1.0;
    return vec4<f32>(w, w, w, w);
}
