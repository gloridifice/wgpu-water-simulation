// Appended to seabed.wgsl; draw the same bottom-attached footprints used by SWE.
struct ColliderOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
}
@vertex
fn collider_vertex(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> ColliderOut {
    if !shore_active(shore) || instance>=u32(shore.collider_meta.x) {
        return ColliderOut(vec4(2.0,2.0,2.0,1.0),vec3(0.0),vec3(0.0,1.0,0.0));
    }
    let c=shore.colliders[instance];
    var local=vec3(0.0); var normal=vec3(0.0,1.0,0.0);
    if c.solid.x<1.5 {
        let segment=index/9u; let vertex=index%9u;
        let a=f32(segment)*6.28318530718/64.0; let b=f32(segment+1u)*6.28318530718/64.0;
        let pa=vec2(cos(a),sin(a)); let pb=vec2(cos(b),sin(b));
        if vertex<3u {
            let cap=array<vec2<f32>,3>(vec2(0.0),pa,pb);
            local=vec3(cap[vertex].x,1.0,cap[vertex].y);
        } else {
            let side=array<vec3<f32>,6>(vec3(pa.x,0.0,pa.y),vec3(pb.x,0.0,pb.y),vec3(pa.x,1.0,pa.y),
                vec3(pa.x,1.0,pa.y),vec3(pb.x,0.0,pb.y),vec3(pb.x,1.0,pb.y));
            local=side[vertex-3u]; normal=vec3(local.x,0.0,local.z);
        }
    } else {
        if index>=36u { return ColliderOut(vec4(2.0,2.0,2.0,1.0),vec3(0.0),normal); }
        let corners=array<vec3<f32>,8>(vec3(-1.0,0.0,-1.0),vec3(1.0,0.0,-1.0),vec3(1.0,0.0,1.0),vec3(-1.0,0.0,1.0),
            vec3(-1.0,1.0,-1.0),vec3(1.0,1.0,-1.0),vec3(1.0,1.0,1.0),vec3(-1.0,1.0,1.0));
        let indices=array<u32,36>(0u,4u,1u,1u,4u,5u, 1u,5u,2u,2u,5u,6u,
            2u,6u,3u,3u,6u,7u, 3u,7u,0u,0u,7u,4u, 4u,7u,5u,5u,7u,6u, 0u,1u,3u,3u,1u,2u);
        let normals=array<vec3<f32>,6>(vec3(0.0,0.0,-1.0),vec3(1.0,0.0,0.0),vec3(0.0,0.0,1.0),
            vec3(-1.0,0.0,0.0),vec3(0.0,1.0,0.0),vec3(0.0,-1.0,0.0));
        local=corners[indices[index]]; normal=normals[index/6u];
    }
    let scaled=local.xz*c.footprint.zw;
    let xz=c.footprint.xy+vec2(c.solid.z*scaled.x-c.solid.w*scaled.y,c.solid.w*scaled.x+c.solid.z*scaled.y);
    let extent_x=abs(c.solid.z)*c.footprint.z+abs(c.solid.w)*c.footprint.w;
    var base=terrain_height(c.footprint.xy,shore)-shore.terrain.y*extent_x-0.1;
    if shore.model.x>0.5 { base=min(terrain_height(xz,shore),c.solid.y-0.1)-0.1; }
    let world=vec3(xz.x,mix(base,c.solid.y,local.y),xz.y);
    normal=vec3(c.solid.z*normal.x-c.solid.w*normal.z,normal.y,c.solid.w*normal.x+c.solid.z*normal.z);
    return ColliderOut(camera.view_projection*vec4(world,1.0),world,normal);
}
@fragment
fn collider_fragment(input: ColliderOut) -> @location(0) vec4<f32> {
    // This first caustic receiver is the seabed plane, not collider surfaces.
    if u32(optics.source_view.w)==12u { return vec4(0.0,0.0,0.0,1.0); }
    let light=0.3+0.7*max(dot(normalize(input.normal),camera.sun.xyz),0.0);
    let wet=mix(0.65,1.0,smoothstep(-0.1,0.2,input.world.y));
    return vec4(vec3(0.38,0.41,0.44)*light*wet,1.0);
}
