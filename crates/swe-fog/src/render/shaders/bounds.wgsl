struct BoundsParameters { reconstruction:vec4f }
@group(0) @binding(0) var<uniform> bounds_parameters:BoundsParameters;
@group(0) @binding(1) var field:texture_2d<f32>;
@group(0) @binding(2) var<storage,read> bed:array<vec4f>;
@group(0) @binding(5) var<storage,read> masks:array<u32>;
@group(0) @binding(7) var smoothed:texture_2d<f32>;
@group(0) @binding(8) var lower_out:texture_storage_2d<rgba32float,write>;
@group(0) @binding(9) var upper_out:texture_storage_2d<rgba32float,write>;
@compute @workgroup_size(8,8)
fn build_leaves(@builtin(global_invocation_id) id:vec3u) {
    if any(id.xy>=textureDimensions(lower_out)){return;}
    let bounds=compute_layer_bounds(vec2i(id.xy),vec2i(textureDimensions(field)),bounds_parameters.reconstruction.x);
    textureStore(lower_out,vec2i(id.xy),bounds.lower);
    textureStore(upper_out,vec2i(id.xy),bounds.upper);
}
