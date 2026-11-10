@group(0) @binding(0) var field:texture_2d<f32>;
@group(0) @binding(1) var<storage,read> bed:array<vec4f>;
@group(0) @binding(2) var<storage,read> masks:array<u32>;
@group(0) @binding(3) var output:texture_storage_2d<rgba32float,write>;
@group(0) @binding(4) var<uniform> settings:vec4f;
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id:vec3u) {
    let dims=vec2i(textureDimensions(field));let c=vec2i(id.xy);
    if any(c>=dims){return;}
    if bed[c.y*dims.x+c.x].y<0.5{textureStore(output,c,vec4f(0));return;}
    let original=textureLoad(field,c,0).xy;var sum=vec2f(0);var total=0.0;
    for(var z=-1;z<=1;z++){for(var x=-1;x<=1;x++){
        let n=clamp(c+vec2i(x,z),vec2i(0),dims-1);
        if bed[n.y*dims.x+n.x].y>0.5&&linked_path(c,n,dims,true)&&linked_path(c,n,dims,false){
            let w=f32((2-abs(x))*(2-abs(z)));sum+=w*textureLoad(field,n,0).xy;total+=w;
        }
    }}
    textureStore(output,c,vec4f(mix(original,sum/max(total,1.0),settings.x),0,0));
}
