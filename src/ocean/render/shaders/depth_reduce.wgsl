@group(0) @binding(0) var scene: texture_depth_2d;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var destination: texture_storage_2d<rg32float,write>;
@compute @workgroup_size(8,8)
fn base(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=textureDimensions(destination)) { return; }
    var depth=1.0;
    if all(id.xy<textureDimensions(scene)) { depth=textureLoad(scene,vec2<i32>(id.xy),0); }
    textureStore(destination,vec2<i32>(id.xy),vec4(depth,depth,0.0,0.0));
}
@compute @workgroup_size(8,8)
fn reduce(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=textureDimensions(destination)) { return; }
    let high=vec2<i32>(textureDimensions(source))-vec2(1);
    let p=vec2<i32>(id.xy)*2;
    let a=textureLoad(source,min(p,high),0).xy;
    let b=textureLoad(source,min(p+vec2(1,0),high),0).xy;
    let c=textureLoad(source,min(p+vec2(0,1),high),0).xy;
    let d=textureLoad(source,min(p+vec2(1,1),high),0).xy;
    textureStore(destination,vec2<i32>(id.xy),vec4(min(min(a.x,b.x),min(c.x,d.x)),max(max(a.y,b.y),max(c.y,d.y)),0.0,0.0));
}
