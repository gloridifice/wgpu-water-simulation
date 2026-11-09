// Cell-bounded ray integration with mask-aware bilinear XZ reconstruction.
// Vertical 6s(1-s) is integrated analytically for each local segment;
// continuously varying h,m,b are approximated by two segments per grid cell.
struct Camera { inverse:mat4x4f, eye:vec4f, domain:vec4f, player:vec4f, params:vec4f, viewport:vec4f }
@group(0) @binding(0) var<uniform> camera:Camera;
@group(0) @binding(1) var field:texture_2d<f32>;
@group(0) @binding(2) var<storage,read> bed:array<vec4f>;
struct Geometry { header:vec4u, vertices:array<vec4f> }
@group(0) @binding(3) var<storage,read> geometry:Geometry;
@group(0) @binding(4) var<storage,read> control:array<u32>;
@group(0) @binding(5) var<storage,read> masks:array<u32>;
struct Out { @builtin(position) position:vec4f, @location(0) uv:vec2f }
@vertex fn vs(@builtin(vertex_index) i:u32)->Out {
    let p=array<vec2f,3>(vec2f(-1,-1),vec2f(3,-1),vec2f(-1,3));var o:Out;o.position=vec4f(p[i],0,1);o.uv=p[i]*0.5+0.5;return o;
}
fn profile(s:f32)->f32 {let t=clamp(s,0.0,1.0);return t*t*(3.0-2.0*t);}
fn layer_integral(y0:f32,dy:f32,h:f32,dist:f32)->f32 {
    if h<=0.0001{return 0.0;}
    if abs(dy)<0.00001 {let s=y0/h;return select(0.0,6.0*s*(1.0-s)*dist,s>0.0&&s<1.0);}
    return max(0.0,h*(profile((y0+dy*dist)/h)-profile(y0/h))/dy);
}
fn box_hit(o:vec3f,d:vec3f,lo:vec3f,hi:vec3f)->vec2f {
    let safe=select(d,vec3f(0.000001),abs(d)<vec3f(0.000001));let a=(lo-o)/safe;let b=(hi-o)/safe;let l=min(a,b);let h=max(a,b);return vec2f(max(max(l.x,l.y),l.z),min(min(h.x,h.y),h.z));
}
fn sphere_hit(o:vec3f,d:vec3f,c:vec3f,r:f32)->f32 {let q=o-c;let b=dot(q,d);let h=b*b-dot(q,q)+r*r;if h<0{return 1e20;}let t=-b-sqrt(h);if t>0{return t;}return 1e20;}
fn capsule_hit(o:vec3f,d:vec3f,feet:vec3f)->f32 {
    let r=0.35;let a=feet+vec3f(0,r,0);let b=feet+vec3f(0,1.8-r,0);let ba=b-a;let oa=o-a;let baba=dot(ba,ba);let bard=dot(ba,d);let baoa=dot(ba,oa);let rdoa=dot(d,oa);let oaoa=dot(oa,oa);let aa=baba-bard*bard;let bb=baba*rdoa-baoa*bard;let cc=baba*oaoa-baoa*baoa-r*r*baba;let discriminant=bb*bb-aa*cc;
    var t=1e20;if discriminant>=0&&abs(aa)>1e-6 {let candidate=(-bb-sqrt(discriminant))/aa;let y=baoa+candidate*bard;if candidate>0&&y>0&&y<baba{t=candidate;}}
    return min(t,min(sphere_hit(o,d,a,r),sphere_hit(o,d,b,r)));
}
fn triangle_hit(o:vec3f,d:vec3f,a:vec3f,b:vec3f,c:vec3f)->vec4f {
    let e1=b-a;let e2=c-a;let p=cross(d,e2);let det=dot(e1,p);if abs(det)<1e-8{return vec4f(1e20,0,1,0);}let q=o-a;let u=dot(q,p)/det;let v=dot(d,cross(q,e1))/det;let t=dot(e2,cross(q,e1))/det;if u<0||v<0||u+v>1||t<=0{return vec4f(1e20,0,1,0);}return vec4f(t,normalize(cross(e1,e2)));
}
fn linked(a:vec2i,b:vec2i,dims:vec2i)->bool {
    if a.x!=b.x {let x=max(a.x,b.x);if masks[a.y*(dims.x+1)+x]==0u{return false;}}
    if a.y!=b.y {let offset=(dims.x+1)*dims.y;let z=max(a.y,b.y);if masks[offset+z*dims.x+b.x]==0u{return false;}}return true;
}
fn sample_layer(pos:vec2f,anchor:vec2i,dims:vec2i)->vec3f {
    let coordinate=(pos-camera.domain.xy)/(camera.domain.zw/vec2f(dims))-0.5;let base=vec2i(floor(coordinate));let f=fract(coordinate);var result=vec3f(0);var weights=0.0;
    for(var z=0;z<2;z++){for(var x=0;x<2;x++){let c=clamp(base+vec2i(x,z),vec2i(0),dims-1);let index=c.y*dims.x+c.x;if bed[index].y>0.5&&linked(anchor,c,dims){let w=select(1.0-f.x,f.x,x==1)*select(1.0-f.y,f.y,z==1);let q=textureLoad(field,c,0);result+=vec3f(q.xy,bed[index].x)*w;weights+=w;}}}
    return result/max(weights,0.000001);
}
fn grid_line(pos:vec2f,period:f32,width:f32)->f32 {
    let c=pos/period;let footprint=max(fwidth(c),vec2f(0.00001));let dist=abs(fract(c-0.5)-0.5)/footprint;let line=1.0-smoothstep(width,width+1.0,min(dist.x,dist.y));return line*(1.0-smoothstep(0.15,0.5,max(footprint.x,footprint.y)));
}
fn shade(uv:vec2f)->vec4f {
    let ndc=vec2f(uv.x*2.0-1.0,uv.y*2.0-1.0);let far=camera.inverse*vec4f(ndc,1,1);let origin=camera.eye.xyz;let ray=normalize(far.xyz/far.w-origin);
    let domain=camera.domain;var floor_t=1e20;var ground_normal=vec3f(0,1,0);var is_floor=true;
    for(var i=0u;i<geometry.header.x;i++){let a=geometry.vertices[3u*i];let b=geometry.vertices[3u*i+1u];let c=geometry.vertices[3u*i+2u];let candidate=triangle_hit(origin,ray,a.xyz,b.xyz,c.xyz);if candidate.x<floor_t{floor_t=candidate.x;ground_normal=candidate.yzw;is_floor=a.w<0.5;}}
    let player_t=capsule_hit(origin,ray,camera.player.xyz);let visible_t=min(floor_t,player_t);
    // Evaluate derivatives before divergent surface branches.
    let ground_point=origin+ray*min(floor_t,200.0);let fine=grid_line(ground_point.xz,1.0,0.4);let major=grid_line(ground_point.xz,5.0,0.8);let axis_distance=abs(ground_point.xz)/max(fwidth(ground_point.xz),vec2f(0.0001));
    var color=mix(vec3f(0.48,0.68,0.78),vec3f(0.19,0.32,0.49),clamp(ray.y*2.0+0.1,0.0,1.0));
    if floor_t>0.0&&floor_t<player_t {
        let p=origin+ray*floor_t;
        if all(p.xz>=domain.xy)&&all(p.xz<=domain.xy+domain.zw) {
            color=mix(vec3f(0.22,0.28,0.065),vec3f(0.40,0.48,0.14),fine);color=mix(color,vec3f(0.57,0.64,0.22),major*0.7);
            color=mix(color,vec3f(0.55,0.38,0.10),1.0-smoothstep(1.0,2.0,axis_distance.y));color=mix(color,vec3f(0.26,0.43,0.12),1.0-smoothstep(1.0,2.0,axis_distance.x));
            // Soft analytic player shadow, for a consistent scale cue.
            let shadow=length((p.xz-camera.player.xz)-vec2f(0.35,0.55));color*=1.0-0.26*(1.0-smoothstep(0.15,0.8,shadow));
            if !is_floor{color=vec3f(0.24,0.27,0.22)*(0.35+0.65*abs(dot(ground_normal,normalize(vec3f(-0.3,0.8,0.5)))));}
        }
    }
    if player_t<floor_t {
        let hit=origin+ray*player_t;let center=camera.player.xyz+vec3f(0,clamp(hit.y-camera.player.y,0.35,1.45),0);let normal=normalize(hit-center);let light=0.36+0.64*max(dot(normal,normalize(vec3f(-0.3,0.8,0.5))),0.0);color=vec3f(1.0,0.36,0.045)*light;
    }
    let dims=vec2i(textureDimensions(field));let upper=camera.viewport.w+bitcast<f32>(control[13]);let hit=box_hit(origin,ray,vec3f(domain.x,camera.viewport.z,domain.y),vec3f(domain.x+domain.z,upper,domain.y+domain.w));var t=max(hit.x,0.0);let end=min(min(hit.y,visible_t),64.0);var optical=0.0;
    if camera.params.z>0.5 && end>t {
        let spacing=domain.zw/vec2f(dims);let first=origin+ray*(t+0.000001);
        var cell=clamp(vec2i(floor((first.xz-domain.xy)/spacing)),vec2i(0),dims-1);let step=select(vec2i(-1),vec2i(1),ray.xz>vec2f(0));
        let boundary=domain.xy+(vec2f(cell)+select(vec2f(0),vec2f(1),ray.xz>vec2f(0)))*spacing;
        var edge=vec2f(1e20);var delta=vec2f(1e20);
        if abs(ray.x)>0.0000001{edge.x=(boundary.x-origin.x)/ray.x;delta.x=spacing.x/abs(ray.x);}
        if abs(ray.z)>0.0000001{edge.y=(boundary.y-origin.z)/ray.z;delta.y=spacing.y/abs(ray.z);}
        // Integer DDA advances each crossed face once, including corner ties.
        // Re-flooring a floating point boundary can otherwise revisit a cell.
        for(var iteration=0;iteration<2048&&t<end&&optical<14.0;iteration++) {
            if any(cell<vec2i(0))||any(cell>=dims){break;}
            let index=cell.y*dims.x+cell.x;let next=min(end,max(t,min(edge.x,edge.y)));let span=next-t;
            if bed[index].y>0.5 {
                for(var sub=0;sub<2;sub++){let start=t+span*f32(sub)*0.5;let middle=origin+ray*(start+span*0.25);let layer=sample_layer(middle.xz,cell,dims);if layer.x>0.0001{let y0=origin.y+ray.y*start-layer.z;optical+=camera.params.x*layer.y/layer.x*layer_integral(y0,ray.y,layer.x,span*0.5);}}
            }
            t=next;if t>=end{break;}
            let cross_x=edge.x<=edge.y;let cross_z=edge.y<=edge.x;
            if cross_x{cell.x+=step.x;edge.x+=delta.x;}
            if cross_z{cell.y+=step.y;edge.y+=delta.y;}
        }
    }
    let trans=exp(-min(optical,30.0));color=color*trans+vec3f(camera.params.y)*(1.0-trans);
    if camera.params.w>0.5 && floor_t>0.0 && floor_t<player_t {
        let pos=(origin+ray*floor_t).xz;let cell=vec2i(floor((pos-domain.xy)/(domain.zw/vec2f(dims))));
        if all(cell>=vec2i(0))&&all(cell<dims) {let q=textureLoad(field,cell,0);let index=cell.y*dims.x+cell.x;var v=0.0;let mode=u32(camera.params.w);if mode==1u{v=q.x;}if mode==2u{v=q.y;}if mode==3u{v=length(q.zw)*10.0;}if mode==4u{v=q.x+bed[index].x;}color=vec3f(clamp(v,0.0,1.0));}
    }
    if camera.params.w==5.0 {color=vec3f(1.0-trans);}
    return vec4f(color,1.0);
}
// Along-ray integration alone cannot filter screen-space coverage. Four fixed
// subpixel rays integrate a pixel footprint without temporal jitter/history.
@fragment fn fs(in:Out)->@location(0) vec4f {
    var result=vec4f(0);
    for(var y=0;y<2;y++){for(var x=0;x<2;x++){
        let offset=(vec2f(f32(x),f32(y))-0.5)*0.5/camera.viewport.xy;
        result+=shade(in.uv+offset);
    }}
    return result*0.25;
}
