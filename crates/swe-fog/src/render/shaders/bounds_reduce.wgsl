// Each view exposes exactly one mip: no simultaneous read/write of a subresource.
@group(0) @binding(0) var lower_in:texture_2d<f32>;
@group(0) @binding(1) var upper_in:texture_2d<f32>;
@group(0) @binding(2) var lower_out:texture_storage_2d<rgba32float,write>;
@group(0) @binding(3) var upper_out:texture_storage_2d<rgba32float,write>;
@compute @workgroup_size(8,8)
fn reduce_bounds(@builtin(global_invocation_id) id:vec3u) {
    if any(id.xy>=textureDimensions(lower_out)){return;}
    var lower=vec4f(1e20);var upper=vec4f(-1e20);
    for(var z=0u;z<2u;z++){for(var x=0u;x<2u;x++){
        let c=2u*id.xy+vec2u(x,z);
        // Rectangular pyramids eventually have only one child along one axis.
        if all(c<textureDimensions(lower_in)) {
            lower=min(lower,textureLoad(lower_in,vec2i(c),0));
            upper=max(upper,textureLoad(upper_in,vec2i(c),0));
        }
    }}
    textureStore(lower_out,vec2i(id.xy),lower);
    textureStore(upper_out,vec2i(id.xy),upper);
}
