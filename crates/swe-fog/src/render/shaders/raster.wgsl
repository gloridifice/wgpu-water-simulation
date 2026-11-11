// Projected tetrahedra with affine density, exact interval integration and 4x MSAA.
override FAR_PASS:bool=false;
const SPLIT_DEPTH:f32=8.0;
@group(0) @binding(0) var<uniform> camera:RasterCamera;
struct Geometry { header:vec4u, vertices:array<vec4f> }
@group(0) @binding(1) var<storage,read> geometry:Geometry;
@group(0) @binding(2) var field:texture_2d<f32>;
@group(0) @binding(3) var<storage,read> bed:array<vec4f>;
@group(0) @binding(4) var smoothed:texture_2d<f32>;
@group(0) @binding(5) var<storage,read> tets:array<TetData>;
@group(0) @binding(6) var<storage,read> projected:array<ProjectedTet>;
@group(0) @binding(8) var<storage,read> model_colors:array<vec4f>;
@group(1) @binding(0) var scene_distance:texture_multisampled_2d<f32>;
@group(1) @binding(1) var scene_color:texture_multisampled_2d<f32>;
@group(1) @binding(2) var optical_depth:texture_multisampled_2d<f32>;
fn view_ray(pixel:vec2f)->vec3f {
    let uv=pixel/camera.viewport.xy;
    let p=camera.inverse*vec4f(uv.x*2.0-1.0,1.0-uv.y*2.0,1,1);
    return normalize(p.xyz/p.w-camera.eye.xyz);
}
struct Fullscreen { @builtin(position) position:vec4f, @location(0) @interpolate(linear,sample) pixel:vec2f }
@vertex fn fullscreen(@builtin(vertex_index) i:u32)->Fullscreen {
    let p=array<vec2f,3>(vec2f(-1,-1),vec2f(3,-1),vec2f(-1,3));
    return Fullscreen(vec4f(p[i],0,1),vec2f(p[i].x*0.5+0.5,0.5-p[i].y*0.5)*camera.viewport.xy);
}
struct SceneResult { @location(0) color:vec4f, @location(1) distance:f32 }
@fragment fn sky(in:Fullscreen)->SceneResult {
    let ray=view_ray(in.pixel);
    return SceneResult(vec4f(mix(vec3f(0.48,0.68,0.78),vec3f(0.19,0.32,0.49),clamp(ray.y*2.0+0.1,0.0,1.0)),1),200.0);
}
struct Surface { @builtin(position) position:vec4f, @location(0) @interpolate(perspective,sample) world:vec3f, @location(1) @interpolate(flat) normal:vec3f, @location(2) @interpolate(flat) kind:f32, @location(3) tint:vec3f }
@vertex fn scene_vertex(@builtin(vertex_index) i:u32)->Surface {
    let base=(i/3u)*3u;
    let a=geometry.vertices[base];let b=geometry.vertices[base+1u];let c=geometry.vertices[base+2u];
    let world=geometry.vertices[i].xyz;
    var tint=vec3f(-1);if geometry.header.y!=0u{tint=model_colors[i].rgb;}
    return Surface(camera.view_projection*vec4f(world,1),world,normalize(cross(b.xyz-a.xyz,c.xyz-a.xyz)),a.w,tint);
}
fn grid_line(pos:vec2f,period:f32,width:f32)->f32 {
    let c=pos/period;let footprint=max(fwidth(c),vec2f(0.00001));let dist=abs(fract(c-0.5)-0.5)/footprint;let line=1.0-smoothstep(width,width+1.0,min(dist.x,dist.y));return line*(1.0-smoothstep(0.15,0.5,max(footprint.x,footprint.y)));
}
@fragment fn scene_fragment(in:Surface)->SceneResult {
    let p=in.world;
    let fine=grid_line(p.xz,1.0,0.4);let major=grid_line(p.xz,5.0,0.8);
    let axis_distance=abs(p.xz)/max(fwidth(p.xz),vec2f(0.0001));
    var color=mix(vec3f(0.22,0.28,0.065),vec3f(0.40,0.48,0.14),fine);
    color=mix(color,vec3f(0.57,0.64,0.22),major*0.7);
    color=mix(color,vec3f(0.55,0.38,0.10),1.0-smoothstep(1.0,2.0,axis_distance.y));
    color=mix(color,vec3f(0.26,0.43,0.12),1.0-smoothstep(1.0,2.0,axis_distance.x));
    color*=1.0-0.26*(1.0-smoothstep(0.15,0.8,length((p.xz-camera.player.xz)-vec2f(0.35,0.55))));
    if in.kind>0.5{color=vec3f(0.24,0.27,0.22)*(0.35+0.65*abs(dot(in.normal,normalize(vec3f(-0.3,0.8,0.5)))));}
    if in.tint.x>=0.0 { color=in.tint*(0.65+0.35*abs(dot(in.normal,normalize(vec3f(-0.3,0.8,0.5))))); }
    if camera.params.w>0.5&&camera.params.w<4.5 {
        let dims=vec2i(textureDimensions(field));let cell=vec2i(floor((p.xz-camera.domain.xy)/camera.domain.zw*vec2f(dims)));
        if all(cell>=vec2i(0))&&all(cell<dims){let q=textureLoad(field,cell,0);var value=q.x;
            if camera.params.w==2.0{value=q.y;}if camera.params.w==3.0{value=length(q.zw)*10.0;}if camera.params.w==4.0{value=q.x+bed[cell.y*dims.x+cell.x].x;}
            color=vec3f(clamp(value,0.0,1.0));
        }
    }
    // RGB is opaque color; alpha is a binary height-fog receiver mask, not
    // material transparency. Classify before RGBA16F storage so half-float
    // quantization cannot shift the 45-degree cutoff.
    let fog_receiver=select(0.0,1.0,in.normal.y*in.normal.y>=dot(in.normal.xz,in.normal.xz));
    return SceneResult(vec4f(color,fog_receiver),length(p-camera.eye.xyz));
}
fn sphere_hit(o:vec3f,d:vec3f,c:vec3f,r:f32)->f32 {let q=o-c;let b=dot(q,d);let h=b*b-dot(q,q)+r*r;if h<0{return 1e20;}let t=-b-sqrt(h);if t>0{return t;}return 1e20;}
fn capsule_hit(o:vec3f,d:vec3f,feet:vec3f)->f32 {
    let r=0.35;let a=feet+vec3f(0,r,0);let b=feet+vec3f(0,1.8-r,0);let ba=b-a;let oa=o-a;let baba=dot(ba,ba);let bard=dot(ba,d);let baoa=dot(ba,oa);let rdoa=dot(d,oa);let oaoa=dot(oa,oa);let aa=baba-bard*bard;let bb=baba*rdoa-baoa*bard;let cc=baba*oaoa-baoa*baoa-r*r*baba;let discriminant=bb*bb-aa*cc;
    var t=1e20;if discriminant>=0&&abs(aa)>1e-6 {let candidate=(-bb-sqrt(discriminant))/aa;let y=baoa+candidate*bard;if candidate>0&&y>0&&y<baba{t=candidate;}}
    return min(t,min(sphere_hit(o,d,a,r),sphere_hit(o,d,b,r)));
}
struct CapsuleResult { @location(0) color:vec4f, @location(1) distance:f32, @builtin(frag_depth) depth:f32 }
@fragment fn capsule(in:Fullscreen)->CapsuleResult {
    let ray=view_ray(in.pixel);let t=capsule_hit(camera.eye.xyz,ray,camera.player.xyz);
    if t>=200.0{discard;}
    let p=camera.eye.xyz+ray*t;let center=camera.player.xyz+vec3f(0,clamp(p.y-camera.player.y,0.35,1.45),0);
    let light=0.36+0.64*max(dot(normalize(p-center),normalize(vec3f(-0.3,0.8,0.5))),0.0);
    let clip=camera.view_projection*vec4f(p,1);
    return CapsuleResult(vec4f(vec3f(1.0,0.36,0.045)*light,1),t,clamp(clip.z/clip.w,0.0,1.0));
}
struct Interval { @builtin(position) position:vec4f, @location(0) @interpolate(linear,sample) depths:vec2f, @location(1) @interpolate(linear,sample) pixel:vec2f, @location(2) @interpolate(flat) density:vec4f }
@vertex fn interval_vertex(@builtin(vertex_index) vertex:u32,@builtin(instance_index) instance:u32)->Interval {
    let flags=projected[instance].flags;
    if flags.x!=1u||(FAR_PASS&&bitcast<f32>(flags.z)<=SPLIT_DEPTH)||(!FAR_PASS&&bitcast<f32>(flags.y)>=SPLIT_DEPTH){return Interval(vec4f(2,2,0,1),vec2f(1),vec2f(0),vec4f(0));}
    let proxy=projected[instance];var p=proxy.center;if vertex<4u{p=proxy.hull[vertex];}
    return Interval(vec4f(p.xy,0.5,1),p.zw,vec2f(p.x*0.5+0.5,0.5-p.y*0.5)*camera.viewport.xy,proxy.density);
}
fn optical_interval(ray:vec3f,start:f32,end:f32,density:vec4f)->f32 {
    if end<=start{return 0.0;}
    let slope=dot(density.xyz,ray);let rho0=dot(density.xyz,camera.eye.xyz)+density.w;
    let average=max(0.0,0.5*(max(0.0,rho0+slope*start)+max(0.0,rho0+slope*end)));
    return min(80.0,camera.params.x*average*(end-start));
}
@fragment fn interval_fragment(in:Interval,@builtin(sample_index) sample:u32)->@location(0) f32 {
    let ray=view_ray(in.pixel);
    let forward=vec3f(camera.view_projection[0].w,camera.view_projection[1].w,camera.view_projection[2].w);
    let cosine=dot(forward,ray);
    let split=SPLIT_DEPTH/cosine;
    let start=max(select(0.0,split,FAR_PASS),1.0/(in.depths.x*cosine));
    let end=min(min(min(64.0,select(split,64.0,FAR_PASS)),textureLoad(scene_distance,vec2i(in.position.xy),i32(sample)).x),1.0/(in.depths.y*cosine));
    return optical_interval(ray,start,end,in.density);
}
struct TetFace { @builtin(position) position:vec4f, @location(0) @interpolate(perspective,sample) world:vec3f, @location(1) @interpolate(flat) index:u32 }
@vertex fn fallback_vertex(@builtin(vertex_index) vertex:u32,@builtin(instance_index) instance:u32)->TetFace {
    let flags=projected[instance].flags;
    if flags.x!=2u||(FAR_PASS&&bitcast<f32>(flags.z)<=SPLIT_DEPTH)||(!FAR_PASS&&bitcast<f32>(flags.y)>=SPLIT_DEPTH){return TetFace(vec4f(2,2,0,1),vec3f(0),instance);}
    let faces=array<u32,12>(0,2,1,0,1,3,0,3,2,1,2,3);let world=tets[instance].p[faces[vertex]].xyz;
    var clip=camera.view_projection*vec4f(world,1);clip.z=0.5*clip.w;
    return TetFace(clip,world,instance);
}
@fragment fn fallback_fragment(in:TetFace,@builtin(sample_index) sample:u32)->@location(0) f32 {
    let ray=normalize(in.world-camera.eye.xyz);let tet=tets[in.index];
    let a=tet.p[1].xyz-tet.p[0].xyz;let b=tet.p[2].xyz-tet.p[0].xyz;let c=tet.p[3].xyz-tet.p[0].xyz;
    let det=dot(a,cross(b,c));let offset=camera.eye.xyz-tet.p[0].xyz;
    let rows=array<vec3f,3>(cross(b,c)/det,cross(c,a)/det,cross(a,b)/det);
    var value=vec4f(0);var direction=vec4f(0);
    for(var i=0u;i<3u;i++){value[i+1u]=dot(rows[i],offset);direction[i+1u]=dot(rows[i],ray);}
    value.x=1.0-value.y-value.z-value.w;direction.x=-direction.y-direction.z-direction.w;
    let forward=vec3f(camera.view_projection[0].w,camera.view_projection[1].w,camera.view_projection[2].w);let split=SPLIT_DEPTH/dot(forward,ray);
    var start=select(0.0,split,FAR_PASS);var end=min(min(64.0,select(split,64.0,FAR_PASS)),textureLoad(scene_distance,vec2i(in.position.xy),i32(sample)).x);
    for(var i=0u;i<4u;i++){
        if abs(direction[i])<1e-8{if value[i]<0.0{return 0.0;}}
        else{let t=-value[i]/direction[i];if direction[i]>0.0{start=max(start,t);}else{end=min(end,t);}}
    }
    return optical_interval(ray,start,end,projected[in.index].density);
}
// Only skip the far contribution when its maximum remaining transmittance is
// exp(-12) < 6.15e-6. Test each MSAA sample, never a resolved mean opacity.
@fragment fn opacity_mask(in:Fullscreen,@builtin(sample_index) sample:u32)->@builtin(frag_depth) f32 {
    let optical=textureLoad(optical_depth,vec2i(in.position.xy),i32(sample)).x;
    return select(1.0,0.0,optical>=12.0);
}
@fragment fn composite(in:Fullscreen)->@location(0) vec4f {
    let pixel=vec2i(in.position.xy);var color=vec3f(0);
    for(var sample=0;sample<4;sample++){
        let optical=textureLoad(optical_depth,pixel,sample).x;let trans=exp(-min(optical,30.0));
        if camera.params.w==5.0{color+=vec3f(1.0-trans);}
        else{color+=textureLoad(scene_color,pixel,sample).rgb*trans+vec3f(camera.params.y)*(1.0-trans);}
    }
    return vec4f(color*0.25,1);
}
