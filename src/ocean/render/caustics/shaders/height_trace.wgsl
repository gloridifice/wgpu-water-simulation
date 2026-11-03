// Consumer declares height_ranges and the shared terrain/shore bindings.
// Distances are along a unit world-space ray. Status: 0 miss, 1 hit, 2 budget.
struct HeightHit { distance: f32, visits: u32, status: u32 }
fn height_range_offset(n: u32, level: u32) -> u32 {
    let side=n>>level;
    return 4u*(n*n-side*side)/3u;
}
fn height_gap(origin: vec3<f32>, ray: vec3<f32>, distance: f32) -> f32 {
    let p=origin+ray*distance;
    return p.y-model_terrain(p.xz,shore).x;
}
// Within a single bilinear patch the ray/surface difference is quadratic.
// Solve both roots: endpoint sign tests alone miss a thin ridge crossed twice.
fn height_patch_hit(origin: vec3<f32>, ray: vec3<f32>, begin: f32, end: f32) -> f32 {
    let a=height_gap(origin,ray,begin);
    let m=height_gap(origin,ray,(begin+end)*0.5);
    let b=height_gap(origin,ray,end);
    let aa=2.0*(b+a-2.0*m);
    let bb=b-a-aa;
    var root=2.0;
    if abs(a)<0.00002 { return begin; }
    if abs(aa)<0.00001 {
        if abs(bb)>0.000001 { root=-a/bb; }
    } else {
        let discriminant=bb*bb-4.0*aa*a;
        if discriminant>=0.0 {
            let q=-0.5*(bb+select(-1.0,1.0,bb>=0.0)*sqrt(discriminant));
            let r0=q/aa;
            var r1=2.0;
            if abs(q)>1e-12 { r1=a/q; }
            if r0>=-0.00001 && r0<=1.00001 { root=r0; }
            if r1>=-0.00001 && r1<=1.00001 { root=min(root,r1); }
        }
    }
    if root>=-0.00001 && root<=1.00001 { return mix(begin,end,clamp(root,0.0,1.0)); }
    return -1.0;
}
fn height_leaf_hit(origin: vec3<f32>, ray: vec3<f32>, q: vec2<f32>, dq: vec2<f32>, cell: vec2<u32>, begin: f32, end: f32) -> f32 {
    // Split at sample-center boundaries so every interval is one bilinear patch.
    var cuts=vec2(end);
    for(var axis=0u;axis<2u;axis++) {
        if abs(dq[axis])>1e-10 {
            cuts[axis]=clamp((f32(cell[axis])+0.5-q[axis])/dq[axis],begin,end);
        }
    }
    let stops=array<f32,4>(begin,min(cuts.x,cuts.y),max(cuts.x,cuts.y),end);
    for(var i=0u;i<3u;i++) {
        if stops[i+1u]-stops[i]<1e-7 { continue; }
        let hit=height_patch_hit(origin,ray,stops[i],stops[i+1u]);
        if hit>=0.0 { return hit; }
    }
    return -1.0;
}
fn trace_heightfield(origin: vec3<f32>, ray: vec3<f32>, limit: f32) -> HeightHit {
    let n=u32(shore.options.z);
    let q=(origin.xz-shore.domain.xy)/shore.domain.zw*f32(n);
    let dq=ray.xz/shore.domain.zw*f32(n);
    var enter=0.0; var leave=limit;
    for(var axis=0u;axis<2u;axis++) {
        if abs(dq[axis])<1e-10 {
            if q[axis]<0.0 || q[axis]>f32(n) { return HeightHit(-1.0,0u,0u); }
        } else {
            let a=-q[axis]/dq[axis]; let b=(f32(n)-q[axis])/dq[axis];
            enter=max(enter,min(a,b)); leave=min(leave,max(a,b));
        }
    }
    if enter>=leave { return HeightHit(-1.0,0u,0u); }
    let top=u32(firstLeadingBit(n));
    var level=top; var t=enter;
    for(var visit=0u;visit<256u;visit++) {
        if t>=leave { return HeightHit(-1.0,visit,0u); }
        let width=f32(1u<<level);
        let position=clamp(q+dq*t+sign(dq)*0.0002,vec2(0.0),vec2(f32(n)-0.0001));
        let cell=vec2<u32>(floor(position/width));
        let side=n>>level;
        var end=leave;
        for(var axis=0u;axis<2u;axis++) {
            if abs(dq[axis])>1e-10 {
                let boundary=(f32(cell[axis])+select(0.0,1.0,dq[axis]>0.0))*width;
                end=min(end,(boundary-q[axis])/dq[axis]);
            }
        }
        let bounds=height_ranges[height_range_offset(n,level)+cell.y*side+cell.x];
        let y0=origin.y+ray.y*t; let y1=origin.y+ray.y*end;
        let possible=min(y0,y1)<=bounds.y+0.00002 && max(y0,y1)>=bounds.x-0.00002;
        if possible && level>0u { level-=1u; continue; }
        if possible {
            let hit=height_leaf_hit(origin,ray,q,dq,cell,t,end);
            if hit>=0.0 { return HeightHit(hit,visit+1u,1u); }
        }
        t=max(end,t+max(0.000001,abs(t)*0.0000002));
        level=min(level+1u,top);
    }
    return HeightHit(-1.0,256u,2u);
}
