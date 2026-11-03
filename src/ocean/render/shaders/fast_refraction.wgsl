struct FastRefractionHit { world:vec3<f32>, distance:f32, uv:vec2<f32>, valid:bool }
// One local horizontal receiver estimate, not a traversal of scene geometry.
fn fast_refraction_hit(origin:vec3<f32>, receiver:vec3<f32>, incoming:vec3<f32>, ray:vec3<f32>, normal:vec3<f32>) -> FastRefractionHit {
    let miss=FastRefractionHit(receiver,0.0,vec2(0.0),false);
    let thickness=max(origin.y-receiver.y,0.0);
    if thickness<0.008 || ray.y>=-0.01 { return miss; }
    let path=thickness/max(-ray.y,0.15);
    if path>optics.refraction.z { return miss; }
    let start=camera.view_projection*vec4(origin,1.0);
    let end=camera.view_projection*vec4(origin+ray*path,1.0);
    if start.w<=0.0 || end.w<=0.0 { return miss; }
    let base=vec2(start.x/start.w*0.5+0.5,0.5-start.y/start.w*0.5);
    let projected=vec2(end.x/end.w*0.5+0.5,0.5-end.y/end.w*0.5);
    let angle=smoothstep(0.03,0.25,max(dot(-incoming,normal),0.0));
    let weight=optics.fast_refraction.x*smoothstep(0.0,optics.fast_refraction.y,thickness)*angle;
    let size=vec2<f32>(textureDimensions(scene_depth));
    var offset=(projected-base)*size/size.y*weight;
    offset*=min(1.0,optics.fast_refraction.z/max(length(offset),1e-8));
    let uv=base+offset*size.y/size;
    if any(uv<vec2(0.0)) || any(uv>=vec2(1.0)) { return miss; }
    let pixel=vec2<i32>(uv*size);
    let depth=textureLoad(scene_depth,pixel,0);
    if depth>=1.0 || depth<=start.z/start.w { return miss; }
    let opaque=screen_world(pixel,depth);
    let distance=length(opaque-origin);
    if opaque.y>=origin.y-0.008 || distance>optics.refraction.z { return miss; }
    return FastRefractionHit(opaque,distance,uv,true);
}
// Depth-masked bilinear reconstruction keeps subpixel motion smooth without
// borrowing foreground colors from the other side of a rock silhouette.
fn fast_refraction_color(uv:vec2<f32>, surface_depth:f32, fallback:vec3<f32>) -> vec3<f32> {
    let size=vec2<i32>(textureDimensions(scene_depth));
    let grid=uv*vec2<f32>(size)-vec2(0.5);
    let base=vec2<i32>(floor(grid));let f=fract(grid);
    var color=vec3(0.0);
    for(var z=0;z<2;z++) { for(var x=0;x<2;x++) {
        let pixel=base+vec2(x,z);
        let weight=select(1.0-f.x,f.x,x==1)*select(1.0-f.y,f.y,z==1);
        var sample=fallback;
        if all(pixel>=vec2(0)) && all(pixel<size) {
            let depth=textureLoad(scene_depth,pixel,0);
            if depth>surface_depth && depth<1.0 { sample=textureLoad(scene_color,pixel,0).rgb; }
        }
        color+=sample*weight;
    }}
    return color;
}
