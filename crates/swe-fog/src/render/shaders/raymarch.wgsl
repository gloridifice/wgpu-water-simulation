// Diagnostic overrides are compile-time, not per-pixel branches. Normal rendering uses 2.
override ACCELERATION:u32=2u;

// Stackless hierarchical XZ DDA. Only newly entered quadtree nodes are tested;
// crossing a leaf face ascends to the highest changed cell-index bit rather
// than rewalking from the root for every occupied cell. Leaf quadrature is
// unchanged: a hierarchy node may skip an interval, never integrate it coarsely.
fn grid_edges(origin:vec3f,ray:vec3f,boundary:vec2i,spacing:vec2f)->vec2f {
    let world=camera.domain.xy+vec2f(boundary)*spacing;
    var edge=vec2f(1e20);
    if abs(ray.x)>0.0000001{edge.x=(world.x-origin.x)/ray.x;}
    if abs(ray.z)>0.0000001{edge.y=(world.y-origin.z)/ray.z;}
    return edge;
}
// Unchanged leaf DDA reference for staged timing and image-equivalence checks.
// It shares coverage and bounds code; only traversal/precomputation differ.
fn integrate_ray_linear(origin:vec3f,ray:vec3f,start:f32,end:f32,dims:vec2i)->f32 {
    let segments=u32(clamp(camera.eye.w,2.0,32.0));
    let spacing=camera.domain.zw/vec2f(dims);let first=origin+ray*(start+0.000001);
    var cell=clamp(vec2i(floor((first.xz-camera.domain.xy)/spacing)),vec2i(0),dims-1);
    let step=select(vec2i(-1),vec2i(1),ray.xz>vec2f(0));
    var edge=grid_edges(origin,ray,cell+select(vec2i(0),vec2i(1),ray.xz>vec2f(0)),spacing);
    var delta=vec2f(1e20);
    if abs(ray.x)>0.0000001{delta.x=spacing.x/abs(ray.x);}
    if abs(ray.z)>0.0000001{delta.y=spacing.y/abs(ray.z);}
    var t=start;var optical=0.0;
    for(var iteration=0;iteration<2048&&t<end&&optical<14.0;iteration++) {
        if any(cell<vec2i(0))||any(cell>=dims){break;}
        let index=cell.y*dims.x+cell.x;let next=min(end,max(t,min(edge.x,edge.y)));let span=next-t;
        if bed[index].y>0.5&&span>0.0 {
            let bounds=cell_layer_bounds(cell,dims);
            if segment_may_contain_fog(origin.y+ray.y*t,ray.y,span,bounds) {
                let segment_span=span/f32(segments);
                for(var sub=0u;sub<segments;sub++) {
                    optical+=integrate_segment(origin,ray,t+segment_span*f32(sub),segment_span,cell,dims,bounds);
                }
            }
        }
        t=next;if t>=end{break;}
        let cross_x=edge.x<=edge.y;let cross_z=edge.y<=edge.x;
        if cross_x{cell.x+=step.x;edge.x+=delta.x;}
        if cross_z{cell.y+=step.y;edge.y+=delta.y;}
    }
    return optical;
}
fn integrate_ray(origin:vec3f,ray:vec3f,start:f32,end:f32,dims:vec2i)->f32 {
    if end<=start{return 0.0;}
    if ACCELERATION<2u{return integrate_ray_linear(origin,ray,start,end,dims);}
    let segments=u32(clamp(camera.eye.w,2.0,32.0));
    let spacing=camera.domain.zw/vec2f(dims);
    let first=origin+ray*(start+0.000001);
    var cell=clamp(vec2i(floor((first.xz-camera.domain.xy)/spacing)),vec2i(0),dims-1);
    let positive=ray.xz>vec2f(0);
    let step=select(vec2i(-1),vec2i(1),positive);
    let side=select(vec2i(0),vec2i(1),positive);
    var edge=grid_edges(origin,ray,cell+side,spacing);
    var delta=vec2f(1e20);
    if abs(ray.x)>0.0000001{delta.x=spacing.x/abs(ray.x);}
    if abs(ray.z)>0.0000001{delta.y=spacing.y/abs(ray.z);}
    let root=textureNumLevels(bounds_lower)-1u;
    let projected=abs(ray.xz)*(end-start)/spacing;
    let use_tree=max(projected.x,projected.y)>=8.0;
    var level=select(0u,root,use_tree);var t=start;var optical=0.0;
    // A straight XZ ray crosses at most nx+nz cells. Each iteration either
    // advances a leaf face or skips an entire node; descent is bounded by root.
    for(var iteration=0;iteration<2048&&t<end&&optical<14.0;iteration++) {
        if any(cell<vec2i(0))||any(cell>=dims){break;}
        let previous=cell;
        var skip=false;var node_edge=edge;var low=cell;var high=cell+1;
        // Occupied 4x4 tiles use the cheap leaf DDA. Avoid doubling bound
        // fetches at every occupied leaf, and do not traverse a tree for short rays.
        while level>=2u {
            let scale=1i<<level;let node=cell/scale;
            low=node*scale;high=min(low+scale,dims);
            node_edge=grid_edges(origin,ray,select(low,high,positive),spacing);
            let node_end=min(end,max(t,min(node_edge.x,node_edge.y)));
            let bounds=LayerBounds(textureLoad(bounds_lower,node,i32(level)),textureLoad(bounds_upper,node,i32(level)));
            if !segment_may_contain_fog(origin.y+ray.y*t,ray.y,node_end-t,bounds){skip=true;break;}
            level-=1u;
        }
        if skip {
            t=min(end,max(t,min(node_edge.x,node_edge.y)));
            if t>=end{break;}
            let coordinate=(origin.xz+ray.xz*t-camera.domain.xy)/spacing;
            // At a negative-going exact face, the next cell is ceil(x)-1.
            // Set crossed axes from INTEGER node faces, not a nudged ray point.
            cell=clamp(select(vec2i(ceil(coordinate))-1,vec2i(floor(coordinate)),ray.xz>=vec2f(0)),low,high-1);
            if node_edge.x<=node_edge.y{cell.x=select(low.x-1,high.x,positive.x);}
            if node_edge.y<=node_edge.x{cell.y=select(low.y-1,high.y,positive.y);}
            edge=grid_edges(origin,ray,cell+side,spacing);
        } else {
            let next=min(end,max(t,min(edge.x,edge.y)));let span=next-t;
            let bounds=cell_layer_bounds(cell,dims);
            if span>0.0&&segment_may_contain_fog(origin.y+ray.y*t,ray.y,span,bounds) {
                let segment_span=span/f32(segments);
                for(var sub=0u;sub<segments;sub++) {
                    optical+=integrate_segment(origin,ray,t+segment_span*f32(sub),segment_span,cell,dims,bounds);
                }
            }
            t=next;if t>=end{break;}
            let cross_x=edge.x<=edge.y;let cross_z=edge.y<=edge.x;
            if cross_x{cell.x+=step.x;edge.x+=delta.x;}
            if cross_z{cell.y+=step.y;edge.y+=delta.y;}
        }
        if any(cell<vec2i(0))||any(cell>=dims){break;}
        let changed=vec2u(cell)^vec2u(previous);let bits=changed.x|changed.y;
        level=0u;
        if use_tree&&bits!=0u{level=min(root,firstLeadingBit(bits));}
    }
    return optical;
}
