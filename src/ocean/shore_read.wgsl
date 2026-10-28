// Consumers declare `shore` and `shore_field` at their own binding locations.
fn shore_field_at(p: vec2<i32>) -> vec4<f32> {
    let n = i32(shore.options.z);
    if any(p < vec2(0)) || any(p >= vec2(n)) { return vec4(0.0); }
    return shore_field[u32(p.y * n + p.x)];
}
fn sample_shore_history(p: vec2<f32>) -> vec4<f32> {
    if !shore_active(shore) { return vec4(0.0); }
    let grid = (p - shore.domain.xy) / shore.domain.zw * shore.options.z - vec2(0.5);
    let q = vec2<i32>(floor(grid));
    let f = fract(grid);
    return mix(mix(shore_field_at(q), shore_field_at(q + vec2(1, 0)), f.x),
        mix(shore_field_at(q + vec2(0, 1)), shore_field_at(q + vec2(1, 1)), f.x), f.y);
}

// Read N/R without changing the total-density contract of existing consumers.
fn shore_species_at(p: vec2<i32>) -> vec2<f32> {
    let n=i32(shore.options.z);
    if any(p<vec2(0)) || any(p>=vec2(n)) { return vec2(0.0); }
    let i=u32(p.y*n+p.x);
    let total=max(0.0,shore_field[i].x);
    let residual=select(0.0,clamp(shore_field[u32(4*n*n)+i].w,0.0,total),shore.foam_lifetime.x>0.5);
    return vec2(total-residual,residual);
}
fn sample_shore_species(p: vec2<f32>) -> vec2<f32> {
    if !shore_active(shore) { return vec2(0.0); }
    let grid=(p-shore.domain.xy)/shore.domain.zw*shore.options.z-vec2(0.5);
    let q=vec2<i32>(floor(grid)); let f=fract(grid);
    return mix(mix(shore_species_at(q),shore_species_at(q+vec2(1,0)),f.x),
        mix(shore_species_at(q+vec2(0,1)),shore_species_at(q+vec2(1,1)),f.x),f.y);
}
fn shore_extra_at(p: vec2<i32>) -> vec4<f32> {
    let n=i32(shore.options.z);
    if any(p<vec2(0)) || any(p>=vec2(n)) { return vec4(0.0); }
    return shore_field[u32(2*n*n+p.y*n+p.x)];
}
fn sample_shore_extra(p: vec2<f32>) -> vec4<f32> {
    if !shore_active(shore) { return vec4(0.0); }
    let grid=(p-shore.domain.xy)/shore.domain.zw*shore.options.z-vec2(0.5);
    let q=vec2<i32>(floor(grid)); let f=fract(grid);
    return mix(mix(shore_extra_at(q),shore_extra_at(q+vec2(1,0)),f.x),
        mix(shore_extra_at(q+vec2(0,1)),shore_extra_at(q+vec2(1,1)),f.x),f.y);
}
fn shore_sample(p: vec2<f32>, s: ShoreParams) -> ShoreSample {
    let extra=sample_shore_extra(p);
    var height=extra.z;
    if (s.collider_meta.x>0.5 || s.model.x>0.5) && shore_active(s) {
        // A dry obstacle's top is not a water surface. Do not interpolate that
        // elevation into neighboring wet vertices and manufacture a water mound.
        let grid=(p-s.domain.xy)/s.domain.zw*s.options.z-vec2(0.5);
        let base=vec2<i32>(floor(grid)); let f=fract(grid);
        var sum=0.0; var weight=0.0;
        for(var y=0;y<2;y++) { for(var x=0;x<2;x++) {
            let c=base+vec2(x,y);
            if shore_field_at(c).z<=0.00001 { continue; }
            let w=select(1.0-f.x,f.x,x==1)*select(1.0-f.y,f.y,y==1);
            sum+=w*shore_extra_at(c).z; weight+=w;
        }}
        height=sum/max(weight,0.000001);
    }
    return ShoreSample(height*(1.0-ocean_weight(p,s)),vec2(sample_shore_history(p).w,extra.w),0.0);
}
// Packed mip chain, bilinear within a level and trilinear between levels.
fn lace_at(p: vec2<i32>, level: u32) -> f32 {
    let base=u32(shore_lace[0]);
    let size=i32(base>>level);
    let q=((p%vec2(size))+vec2(size))%vec2(size);
    var offset=4u;
    for(var i=0u;i<level;i++) { let w=base>>i; offset+=w*w; }
    return shore_lace[offset+u32(q.y*size+q.x)];
}
fn lace_level(uv: vec2<f32>, level: u32) -> f32 {
    let size=f32(u32(shore_lace[0])>>level);
    let p=fract(uv)*size-vec2(0.5);
    let q=vec2<i32>(floor(p)); let f=fract(p);
    return mix(mix(lace_at(q,level),lace_at(q+vec2(1,0),level),f.x),
        mix(lace_at(q+vec2(0,1),level),lace_at(q+vec2(1,1),level),f.x),f.y);
}
fn lace_sample(uv: vec2<f32>) -> f32 {
    let size=shore_lace[0];
    let footprint=max(length(dpdx(uv)*size),length(dpdy(uv)*size));
    let lod=clamp(log2(max(footprint,1.0)),0.0,log2(size));
    let low=u32(floor(lod)); let high=min(low+1u,u32(log2(size)));
    return mix(lace_level(uv,low),lace_level(uv,high),fract(lod));
}
struct FoamTextureSample { offsets: vec4<f32>, blend: f32 }
fn sample_shore_texture(p: vec2<f32>) -> FoamTextureSample {
    let n=i32(shore.options.z); let cells=u32(n*n);
    let grid=(p-shore.domain.xy)/shore.domain.zw*shore.options.z-vec2(0.5);
    let base=vec2<i32>(floor(grid)); let f=fract(grid);
    var value=vec4(0.0); var weight=0.0;
    for (var y=0; y<2; y++) { for (var x=0; x<2; x++) {
        let c=base+vec2(x,y);
        if any(c<vec2(0)) || any(c>=vec2(n)) { continue; }
        let i=u32(c.y*n+c.x);
        if shore_field[cells+i].x<=shore.density.z { continue; }
        let w=select(1.0-f.x,f.x,x==1)*select(1.0-f.y,f.y,y==1);
        value+=w*vec4(shore_field[3u*cells+i].zw,shore_field[4u*cells+i].xy);
        weight+=w;
    }}
    let c=clamp(base,vec2(0),vec2(n-1));
    let phase=shore_field[4u*cells+u32(c.y*n+c.x)].z;
    // A is invisible at its reset, B at its half-cycle reset; smooth endpoints.
    let blend=select(smoothstep(0.0,1.0,1.0-abs(2.0*phase-1.0)),1.0,shore.foam_shading.w==0.0);
    return FoamTextureSample(value/max(weight,0.000001),blend);
}
