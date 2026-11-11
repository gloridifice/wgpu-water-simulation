// Local horizontal slab approximation, not ray marching or tetrahedron integration.
@group(0) @binding(7) var<storage,read> masks:array<u32>;
fn height_weights(t:f32)->vec4f {
    let t2=t*t;let t3=t2*t;let s=1.0-t;
    return vec4f(s*s*s,4.0-6.0*t2+3.0*t3,1.0+3.0*t+3.0*t2-3.0*t3,t3)/6.0;
}
fn height_layer(pos:vec2f)->vec3f {
    let dims=vec2i(textureDimensions(field));
    let cell=(pos-camera.domain.xy)/camera.domain.zw*vec2f(dims);
    let anchor=vec2i(floor(cell));
    if any(anchor<vec2i(0))||any(anchor>=dims){return vec3f(0);}
    if bed[anchor.y*dims.x+anchor.x].y<0.5{return vec3f(0);}
    let coord=cell-0.5;let base=vec2i(floor(coord));
    let wx=height_weights(fract(coord.x));let wz=height_weights(fract(coord.y));
    var result=vec3f(0);var weight=0.0;
    for(var z=0;z<4;z++){for(var x=0;x<4;x++){
        let c=clamp(base+vec2i(x-1,z-1),vec2i(0),dims-1);
        let b=bed[c.y*dims.x+c.x];
        if b.y>0.5&&linked_path(anchor,c,dims,true)&&linked_path(anchor,c,dims,false){
            let w=wx[x]*wz[z];result+=vec3f(textureLoad(field,c,0).xy,b.x)*w;weight+=w;
        }
    }}
    return result/max(weight,1e-6);
}
fn height_hash(p:vec2f)->f32 {return fract(sin(dot(p,vec2f(127.1,311.7)))*43758.5453);}
fn height_noise(p:vec2f)->f32 {
    let b=floor(p);let f=smoothstep(vec2f(0),vec2f(1),fract(p));
    return 2.0*mix(mix(height_hash(b),height_hash(b+vec2f(1,0)),f.x),mix(height_hash(b+vec2f(0,1)),height_hash(b+vec2f(1)),f.x),f.y)-1.0;
}
fn height_optical(ray:vec3f,distance:f32)->f32 {
    // No sky fog in this surface-anchored approximation. Near-horizontal rays
    // are bounded rather than pretending to recover an accurate side silhouette.
    if distance>=199.9||camera.params.z<0.5||(camera.params.w!=0.0&&camera.params.w!=5.0){return 0.0;}
    let surface=camera.eye.xyz+ray*distance;
    let layer=height_layer(surface.xz);let h=layer.x;
    if h<=0.0001||layer.y<=0.0{return 0.0;}
    // Clip the local vertical slab to camera and opaque surface heights. This
    // prevents fog behind a foreground surface from covering that surface.
    let lower=max(layer.z,min(surface.y,camera.eye.y));
    let upper=min(layer.z+h,max(surface.y,camera.eye.y));
    let length_approx=min(distance,max(0.0,upper-lower)/max(abs(ray.y),0.15));
    // m is mass per area: m/h is visual volume density, not m*h extinction.
    let density=layer.y/h*(1.0+camera.eye.w*height_noise(surface.xz*0.25));
    return max(0.0,camera.params.x*density*length_approx);
}
@fragment fn height_accumulate(in:Fullscreen, @builtin(sample_index) sample:u32)->@location(0) f32 {
    let pixel=vec2i(in.position.xy);
    let offsets=array<vec2f,4>(vec2f(0.375,0.125),vec2f(0.875,0.375),vec2f(0.125,0.625),vec2f(0.625,0.875));
    if textureLoad(scene_color,pixel,i32(sample)).a<=0.5 {return 0.0;}
    return height_optical(view_ray(vec2f(pixel)+offsets[sample]),textureLoad(scene_distance,pixel,i32(sample)).x);
}
