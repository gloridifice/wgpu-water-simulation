struct Camera { matrix:mat4x4f, params:vec4f }
@group(0) @binding(0) var<uniform> camera:Camera;
struct Vertex { @builtin(position) clip:vec4f, @location(0) world:vec3f, @location(1) color:vec3f, @location(2) bary:vec3f, @location(3) @interpolate(flat) kind:f32 }
@vertex fn vs(@location(0) p:vec4f,@location(1) color:vec4f,@builtin(vertex_index) i:u32)->Vertex {
    let bary=array<vec3f,3>(vec3f(1,0,0),vec3f(0,1,0),vec3f(0,0,1));
    return Vertex(camera.matrix*vec4f(p.xyz,1),p.xyz,color.rgb,bary[i%3u],p.w);
}
@fragment fn fs(in:Vertex)->@location(0) vec4f {
    let n=normalize(cross(dpdx(in.world),dpdy(in.world)));
    let light=0.65+0.35*abs(dot(n,normalize(vec3f(-0.3,0.8,0.5))));
    if in.kind<0.5 { return vec4f(in.color*light,1); }
    let edge_width=max(fwidth(in.bary),vec3f(0.00001));
    let inside=smoothstep(vec3f(0.4)*edge_width,vec3f(1.25)*edge_width,in.bary);
    let edge=1.0-min(inside.x,min(inside.y,inside.z));
    if camera.params.x>0.5 {
        var fill=0.10; if in.kind==2.0 {fill=0.42;} if in.kind==3.0 {fill=0.35;}
        return vec4f(in.color,mix(fill,0.95,edge));
    }
    return vec4f(in.color*light*mix(1.0,0.2,edge),1);
}
