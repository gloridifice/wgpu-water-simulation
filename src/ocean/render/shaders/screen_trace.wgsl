struct ScreenHit { world:vec3<f32>, distance:f32, pixel:vec2<i32>, valid:bool }
fn screen_world(pixel:vec2<i32>, depth:f32) -> vec3<f32> {
    let uv=(vec2<f32>(pixel)+vec2(0.5))/vec2<f32>(textureDimensions(scene_depth));
    let h=camera.inverse_view_projection*vec4(uv.x*2.0-1.0,1.0-uv.y*2.0,depth,1.0);
    return h.xyz/h.w;
}
// Traverse projected tiles front-to-back. NDC depth is linear along this
// screen-space segment; reconstruct the visible sample for world ray distance.
fn trace_screen(origin:vec3<f32>, ray:vec3<f32>, maximum:f32, above_water:bool) -> ScreenHit {
    let miss=ScreenHit(vec3(0.0),-1.0,vec2(0),false);
    let start=camera.view_projection*vec4(origin,1.0);
    let delta=camera.view_projection*vec4(ray,0.0);
    var reach=maximum;
    // Clip the ray to the near/far planes and positive homogeneous W.
    if delta.w<0.0 { reach=min(reach,(start.w-0.001)/(-delta.w)); }
    if delta.z<0.0 { reach=min(reach,start.z/(-delta.z)); }
    if delta.w-delta.z<0.0 { reach=min(reach,(start.w-start.z)/(delta.z-delta.w)); }
    if reach<=0.01 || start.w<=0.0 { return miss; }
    let finish=start+delta*reach;
    let a=start.xyz/start.w; let b=finish.xyz/finish.w;
    let size=vec2<f32>(textureDimensions(scene_depth));
    let q=vec2(a.x*0.5+0.5,0.5-a.y*0.5)*size;
    let dq=vec2((b.x-a.x)*0.5,(a.y-b.y)*0.5)*size;
    let dz=b.z-a.z;
    // A depth sample represents a pixel footprint, not an infinitesimal point.
    // Expand bounds conservatively by that footprint in device-depth units.
    let row_x=vec3(camera.view_projection[0].x,camera.view_projection[1].x,camera.view_projection[2].x);
    let row_y=vec3(camera.view_projection[0].y,camera.view_projection[1].y,camera.view_projection[2].y);
    let pixel_angle=length(vec2(2.0/(size.x*length(row_x)),2.0/(size.y*length(row_y))));
    let depth_scale=abs((camera.view_projection*vec4(camera.eye.xyz,1.0)).z);
    let inverse_w_row=vec4(camera.inverse_view_projection[0].w,camera.inverse_view_projection[1].w,camera.inverse_view_projection[2].w,camera.inverse_view_projection[3].w);
    var enter=0.0; var leave=1.0;
    for(var axis=0u;axis<2u;axis++) {
        if abs(dq[axis])<1e-8 {
            if q[axis]<0.0 || q[axis]>=size[axis] { return miss; }
        } else {
            let x=-q[axis]/dq[axis]; let y=(size[axis]-q[axis])/dq[axis];
            enter=max(enter,min(x,y)); leave=min(leave,max(x,y));
        }
    }
    if enter>=leave { return miss; }
    let top=textureNumLevels(scene_ranges)-1u;
    var level=top; var t=enter;
    let budget=select(128u,u32(optics.reflection_trace.x),above_water);
    let tolerance_scale=select(1.0,optics.reflection_trace.y,above_water);
    for(var visit=0u;visit<budget;visit++) {
        if t>=leave { return miss; }
        let width=f32(1u<<level);
        let location=clamp(q+dq*t+sign(dq)*0.001,vec2(0.0),size-vec2(0.0001));
        let cell=vec2<i32>(floor(location/width));
        var end=leave;
        for(var axis=0u;axis<2u;axis++) {
            if abs(dq[axis])>1e-8 {
                let boundary=(f32(cell[axis])+select(0.0,1.0,dq[axis]>0.0))*width;
                end=min(end,(boundary-q[axis])/dq[axis]);
            }
        }
        let range=textureLoad(scene_ranges,cell,i32(level)).xy;
        let z0=a.z+dz*t; let z1=a.z+dz*end;
        let inverse_w=max(dot(inverse_w_row,vec4(a.xy,range.x,1.0)),0.0);
        let padding=max(0.000001,1.5*pixel_angle*depth_scale*inverse_w)*tolerance_scale;
        let possible=range.x<1.0 && min(z0,z1)<=range.y+padding && max(z0,z1)>=range.x-padding;
        if possible && level>0u { level-=1u; continue; }
        if possible {
            let opaque=screen_world(cell,range.x);
            // Project the reconstructed sample onto the world ray. Converting
            // nearly equal far NDC depths back via lambda loses precision.
            let distance=dot(opaque-origin,ray);
            let world=origin+ray*distance;
            let adjacent=screen_world(cell+vec2(1,1),range.x);
            let tolerance=max(0.03,1.5*length(adjacent-opaque))*tolerance_scale;
            if distance>0.01 && distance<=maximum && length(world-opaque)<tolerance {
                // Reflections use the opaque geometry exposed above the local wave.
                if !above_water {
                    return ScreenHit(world,distance,cell,true);
                }
                let grid=(opaque.xz/camera.ocean.y+vec2(0.5))*camera.ocean.x;
                if opaque.y>surface(grid,vec2(0.0)).y {
                    return ScreenHit(world,distance,cell,true);
                }
            }
        }
        t=max(end,t+0.0000001);
        level=min(level+1u,top);
    }
    return miss;
}
