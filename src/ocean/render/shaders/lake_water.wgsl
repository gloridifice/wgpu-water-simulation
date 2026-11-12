// Bounded geometry/mask adapter; all water optics use water::material.
fn lake_foam_hash(p:vec2<f32>) -> vec2<f32> {
    // Integer hashing keeps cell sites identical across pixels and GPU backends.
    let cell=bitcast<vec2<u32>>(vec2<i32>(p));
    var h=cell.x*1597334677u+cell.y*3812015801u;
    h=(h^(h>>16u))*2246822519u;
    h=(h^(h>>13u))*3266489917u;
    h=h^(h>>16u);
    return vec2<f32>(f32(h&65535u),f32(h>>16u))/65536.0;
}
fn lake_foam_noise(p:vec2<f32>) -> f32 {
    let c=floor(p);
    let f=fract(p);
    let u=f*f*(3.0-2.0*f);
    return mix(mix(lake_foam_hash(c).x,lake_foam_hash(c+vec2(1.0,0.0)).x,u.x),
               mix(lake_foam_hash(c+vec2(0.0,1.0)).x,lake_foam_hash(c+vec2(1.0)).x,u.x),u.y);
}
// Distance to the cell boundary with rounded water-hole corners. Unlike F2-F1,
// perpendicular bisectors keep line width stable; smooth intersections thicken junctions.
fn lake_foam_holes(p:vec2<f32>) -> f32 {
    let cell=floor(p);
    let local=fract(p);
    var closest=vec2(0.0);
    var nearest=10.0;
    for(var y=-1;y<=1;y++) {
        for(var x=-1;x<=1;x++) {
            let offset=vec2<f32>(f32(x),f32(y));
            let delta=offset+0.12+0.76*lake_foam_hash(cell+offset)-local;
            let d=dot(delta,delta);
            if d<nearest { nearest=d; closest=delta; }
        }
    }
    var border=2.0;
    for(var y=-2;y<=2;y++) {
        for(var x=-2;x<=2;x++) {
            let offset=vec2<f32>(f32(x),f32(y));
            let delta=offset+0.12+0.76*lake_foam_hash(cell+offset)-local;
            let separation=delta-closest;
            if dot(separation,separation)>0.0001 {
                let d=dot(0.5*(closest+delta),normalize(separation));
                let h=max(0.18-abs(border-d),0.0)/0.18;
                border=min(border,d)-h*h*0.045;
            }
        }
    }
    return border;
}
fn lake_foam_uv(world:vec2<f32>) -> vec2<f32> {
    // Transform both the world position and its displacement by the same metric mapping.
    return vec2(world.x*0.82+world.y*0.22,world.y*1.12-world.x*0.18)/max(lake.foam.z,0.1);
}
fn lake_foam_layer(base:vec2<f32>, band:f32) -> f32 {
    // Static material: all temporal motion comes from the two advected UV domains.
    let drift=vec2(1.0,0.0);
    let warp=vec2(lake_foam_noise(base*0.65+drift*0.25),
                  lake_foam_noise(base*0.65+vec2(7.3,2.1)-drift*0.25));
    let fine=vec2(lake_foam_noise(base*3.8+drift*0.4),
                  lake_foam_noise(base*3.8+vec2(13.7,5.2)-drift*0.4));
    let p=base+2.0*(warp-0.5)+0.18*(fine-0.5)
        +0.13*vec2(sin(base.y*3.0),sin(base.x*2.7));
    let holes=lake_foam_holes(p);
    // Thin outer strands break selectively; thicker inner junctions merge into white patches.
    let detail=lake_foam_noise(p*6.5+drift*0.35);
    let threshold=mix(-0.018,0.035,band)+0.022*sin(p.x*1.7+p.y*2.1)
        +0.035*(detail-0.5);
    let aa=max(length(fwidth(p))*0.45,0.002);
    let sheet=1.0-smoothstep(threshold-aa,threshold+aa,holes);
    let torn=smoothstep(0.18,0.42,lake_foam_noise(p*1.6+drift*0.2)+band*0.2);
    // A weaker, smaller net only near shore adds interlaced strands without whitening
    // the open lake. Both scales are anchored to the same world-space deformation.
    let fine_border=lake_foam_holes(p*1.85+vec2(8.3,2.7));
    let fine_aa=aa*1.85;
    let fine_net=1.0-smoothstep(0.008-fine_aa,0.008+fine_aa,fine_border);
    let fine_coverage=fine_net*band*band*0.32*smoothstep(0.3,0.65,detail);
    return clamp(max(sheet*sqrt(band)*torn,fine_coverage),0.0,1.0);
}
fn lake_foam_offsets(world:vec2<f32>) -> vec4<f32> {
    let grid=clamp((world/lake.grid.y+0.5)*(lake.grid.x-1.0),vec2(0.0),vec2(lake.grid.x-1.001));
    let cell=vec2<i32>(floor(grid));let f=fract(grid);
    var offsets=vec4(0.0);var weight=0.0;
    // Match SWE's wet-only coordinate interpolation. A cleared dry offset is not
    // a valid material donor and must not introduce artificial shore-line shear.
    for(var y=0;y<2;y++) { for(var x=0;x<2;x++) {
        let c=cell+vec2(x,y);
        if textureLoad(lake_terrain,c,0).x>=0.0 { continue; }
        let w=select(1.0-f.x,f.x,x==1)*select(1.0-f.y,f.y,y==1);
        offsets+=w*textureLoad(lake_foam_motion,c,0);weight+=w;
    }}
    return offsets/max(weight,0.000001);
}
fn lake_foam_mask(world:vec3<f32>, shore_distance:f32) -> f32 {
    let base=lake_foam_uv(world.xz);
    let broad=lake_foam_noise(base*0.31+vec2(0.3,0.0));
    let width=max(lake.foam.y,0.1);
    let distance=max(shore_distance,0.0);
    // Coverage stays attached to the shore; only its internal material is advected.
    let reach=width*mix(0.55,1.5,broad)+clamp(world.y*0.8,-0.2,0.2);
    let band=1.0-smoothstep(reach*0.08,reach,distance);
    let contact_width=min(0.28,width*0.08)*mix(0.25,1.5,broad);
    let contact_aa=max(fwidth(distance),0.015);
    let contact=(1.0-smoothstep(contact_width-contact_aa,contact_width+contact_aa,distance))
        *smoothstep(0.12,0.4,lake_foam_noise(base*1.9));

    // Lake centre is XZ=(0,0). Smoothly vanishing inward velocity avoids a centre singularity.
    let flow_direction=-world.xz/sqrt(dot(world.xz,world.xz)+1.0);
    // Only downstream / increasing-distance faces emit the stylized foam. The
    // signed field includes island holes, so rock upstream faces are suppressed.
    // A two-cell stencil smooths the discrete shoreline without changing its mask.
    let step=2.0*lake.grid.y/max(lake.grid.x-1.0,1.0);
    let gradient=vec2(
        lake_sample(lake_terrain,world.xz+vec2(step,0.0)).w-lake_sample(lake_terrain,world.xz-vec2(step,0.0)).w,
        lake_sample(lake_terrain,world.xz+vec2(0.0,step)).w-lake_sample(lake_terrain,world.xz-vec2(0.0,step)).w
    )/(2.0*step);
    let increasing=dot(gradient/max(length(gradient),0.001),flow_direction);
    let downstream=smoothstep(0.0,0.35,increasing);
    let phase_a=lake.foam_flow.z;
    // Like SWE's sample_shore_texture: displacement was integrated along the
    // velocity field, not approximated by the current velocity times layer age.
    let motion=lake_foam_offsets(world.xz);
    let uv_a=lake_foam_uv(world.xz-motion.xy);
    let uv_b=lake_foam_uv(world.xz-motion.zw);
    // A resets only at zero A weight; B resets only at zero B weight. Blend evaluated
    // coverages, not UVs or distance fields, which would slide/backtrack at handoff.
    let triangle=1.0-abs(2.0*phase_a-1.0);
    let weight_a=smoothstep(0.0,1.0,triangle);
    let a=lake_foam_layer(uv_a,band);
    let b=lake_foam_layer(uv_b,band);
    return lake.foam.x*downstream*max(contact,mix(b,a,weight_a));
}
@vertex
fn lake_vertex(@builtin(vertex_index) index:u32) -> WaterOut {
    let xz=lake_position(index);let f=lake_sample(lake_field,xz);
    let p=vec3(xz.x,f.x,xz.y);let normal=normalize(vec3(-f.y,1.0,-f.z));
    return WaterOut(camera.view_projection*vec4(p,1.0),p,normal,vec2(0.0),normal,2u,xz);
}
fn lake_water(input:WaterOut) -> vec4<f32> {
    let terrain=lake_sample(lake_terrain,input.world.xz);
    if terrain.x>=-0.003 { discard; }
    let normal=normalize(input.normal);
    let lighting=shade_water(input.world,normal,water_path(input,normal,false),vec3(0.0));
    // Coverage replaces underlying water radiance rather than adding glowing lines.
    let foam=lake_foam_mask(input.world,terrain.w);
    var color=mix(lighting.color,vec3(0.92,0.97,1.0),foam);
    if lake.options.z>0.5 {
        let c=select(terrain.y,terrain.z,lake.options.y>0.5);
        if lake.options.z<1.5 { color=vec3(terrain.y); }
        else if lake.options.z<2.5 { color=vec3(0.1,0.5,0.9)*clamp(terrain.w/10.0,0.0,1.0); }
        else if lake.options.z<3.5 { color=mix(vec3(0.04,0.16,0.3),vec3(0.8,0.95,1.0),smoothstep(0.35,0.48,abs(fract(c*20.0)-0.5))); }
        else { color=vec3(0.5)+vec3(input.world.y*4.0); }
    }
    return vec4(color,1.0);
}
