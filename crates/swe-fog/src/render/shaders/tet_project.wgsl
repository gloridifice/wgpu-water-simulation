// Projected tetrahedron classification adapted from Interval-Shading singleTet
// mesh.mesh, Thibault Tricard (2024), MIT. See third_party/interval-shading-LICENSE.
// Upstream revision 4b4a256942539fc8a291f13422684a628757d758.
// Compute replaces mesh-shader emission. Near-eye tetrahedra use exact face
// coverage / interval clipping, retaining fog between the eye and near plane.
@group(0) @binding(0) var<uniform> camera:RasterCamera;
@group(0) @binding(5) var<storage,read> tets:array<TetData>;
@group(0) @binding(6) var<storage,read_write> projected:array<ProjectedTet>;
@group(0) @binding(9) var<storage,read_write> draws:array<atomic<u32>>;
fn publish(index:u32,result:ProjectedTet) {
    if result.flags.x==0u{return;}
    projected[index]=result;
    let kind=result.flags.x-1u;let count=u32(camera.viewport.z);
    // Preserve deterministic primitive order; suppress completely empty passes.
    if bitcast<f32>(result.flags.y)<8.0 {atomicStore(&draws[kind*5u+1u],count);}
    if bitcast<f32>(result.flags.z)>8.0 {atomicStore(&draws[(kind+2u)*5u+1u],count);}
}
fn cross2(a:vec2f,b:vec2f)->f32{return a.x*b.y-a.y*b.x;}
@compute @workgroup_size(64)
fn project_tets(@builtin(global_invocation_id) id:vec3u) {
    if id.x>=u32(camera.viewport.z){return;}
    let tet=tets[id.x];var result:ProjectedTet;
    let a=tet.p[1].xyz-tet.p[0].xyz;let b=tet.p[2].xyz-tet.p[0].xyz;let c=tet.p[3].xyz-tet.p[0].xyz;
    let det=dot(a,cross(b,c));
    if det<=1e-12||max(max(tet.p[0].w,tet.p[1].w),max(tet.p[2].w,tet.p[3].w))<=0.0{projected[id.x]=result;return;}
    let gradient=((tet.p[1].w-tet.p[0].w)*cross(b,c)+(tet.p[2].w-tet.p[0].w)*cross(c,a)+(tet.p[3].w-tet.p[0].w)*cross(a,b))/det;
    result.density=vec4f(gradient,tet.p[0].w-dot(gradient,tet.p[0].xyz));
    var clip:array<vec4f,4>;var minw=1e30;var maxw=-1e30;
    for(var i=0u;i<4u;i++){clip[i]=camera.view_projection*vec4f(tet.p[i].xyz,1);minw=min(minw,clip[i].w);maxw=max(maxw,clip[i].w);}
    // Homogeneous side-plane culling is valid even for near-eye tetrahedra.
    for(var plane=0u;plane<4u;plane++){
        var outside=true;for(var i=0u;i<4u;i++){
            let v=clip[i];let distances=vec4f(v.w+v.x,v.w-v.x,v.w+v.y,v.w-v.y);
            outside=outside&&distances[plane]<0.0;
        }
        if outside{projected[id.x]=result;return;}
    }
    result.flags.y=bitcast<u32>(minw);result.flags.z=bitcast<u32>(maxw);
    if maxw<=0.0{projected[id.x]=result;return;}
    if minw<0.05{result.flags.x=2u;publish(id.x,result);return;}
    var p:array<vec3f,4>;var low=vec2f(1e30);var high=vec2f(-1e30);
    for(var i=0u;i<4u;i++){p[i]=vec3f(clip[i].xy/clip[i].w,1.0/clip[i].w);low=min(low,p[i].xy);high=max(high,p[i].xy);}
    if any(low>vec2f(1))||any(high<vec2f(-1)){projected[id.x]=result;return;}
    let faces=array<vec3u,4>(vec3u(1,2,3),vec3u(0,2,3),vec3u(0,1,3),vec3u(0,1,2));
    for(var i=0u;i<4u;i++){
        let f=faces[i];let v0=p[f.x];let v1=p[f.y];let v2=p[f.z];let point=p[i];
        let area=cross2(v1.xy-v0.xy,v2.xy-v0.xy);if abs(area)<1e-12{continue;}
        let l1=cross2(point.xy-v0.xy,v2.xy-v0.xy)/area;let l2=cross2(v1.xy-v0.xy,point.xy-v0.xy)/area;let l0=1.0-l1-l2;
        if min(l0,min(l1,l2))>=0.0 {
            let z=l0*v0.z+l1*v1.z+l2*v2.z;
            result.hull[0]=v0.xyzz;result.hull[1]=v1.xyzz;result.hull[2]=v2.xyzz;result.hull[3]=v0.xyzz;
            result.center=vec4f(point.xy,max(point.z,z),min(point.z,z));result.flags.x=1u;publish(id.x,result);return;
        }
    }
    let crossing=array<vec4u,3>(vec4u(0,1,2,3),vec4u(0,2,1,3),vec4u(0,3,1,2));
    for(var i=0u;i<3u;i++){
        let e=crossing[i];let a0=p[e.x];let a1=p[e.y];let b0=p[e.z];let b1=p[e.w];
        let va=a1.xy-a0.xy;let vb=b1.xy-b0.xy;let det2=cross2(va,vb);if abs(det2)<1e-12{continue;}
        let delta=b0.xy-a0.xy;let ta=cross2(delta,vb)/det2;let tb=cross2(delta,va)/det2;
        if min(ta,tb)>=0.0&&max(ta,tb)<=1.0 {
            let za=mix(a0.z,a1.z,ta);let zb=mix(b0.z,b1.z,tb);
            result.hull[0]=a0.xyzz;result.hull[1]=b0.xyzz;result.hull[2]=a1.xyzz;result.hull[3]=b1.xyzz;
            result.center=vec4f(a0.xy+ta*va,max(za,zb),min(za,zb));result.flags.x=1u;publish(id.x,result);return;
        }
    }
    // Numerically degenerate projection: exact convex-face fallback.
    result.flags.x=2u;publish(id.x,result);
}
