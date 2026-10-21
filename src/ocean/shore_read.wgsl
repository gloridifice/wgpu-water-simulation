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
    return ShoreSample(extra.z*(1.0-ocean_weight(p,s)),vec2(sample_shore_history(p).w,extra.w),0.0);
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
fn flowing_lace(p: vec2<f32>, velocity: vec2<f32>, scale: f32) -> f32 {
    let phase=fract(shore.timing.x*0.5+shore_noise(p/9.7));
    let other=fract(phase+0.5);
    let displacement=velocity*min(1.0,0.35/max(length(velocity),0.0001));
    return mix(lace_sample((p-displacement*(phase-0.5))/scale),
        lace_sample((p-displacement*(other-0.5))/scale),abs(phase*2.0-1.0));
}
