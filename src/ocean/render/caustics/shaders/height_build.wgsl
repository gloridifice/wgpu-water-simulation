@group(0) @binding(0) var<uniform> params: vec4<u32>;
@group(0) @binding(1) var<storage,read> bed: array<vec2<f32>>;
@group(0) @binding(2) var<storage,read_write> bounds: array<vec2<f32>>;
fn offset(level: u32) -> u32 {
    let side=params.x>>level;
    return 4u*(params.x*params.x-side*side)/3u;
}
@compute @workgroup_size(8,8)
fn base(@builtin(global_invocation_id) id: vec3<u32>) {
    let n=params.x;
    if any(id.xy>=vec2(n)) { return; }
    var range=vec2(1e30,-1e30);
    // Samples live at cell centers. A spatial leaf straddles four bilinear
    // patches, so its conservative envelope includes a 3x3 sample halo.
    for(var z=-1;z<=1;z++) { for(var x=-1;x<=1;x++) {
        let p=vec2<u32>(clamp(vec2<i32>(id.xy)+vec2(x,z),vec2(0),vec2(i32(n)-1)));
        let h=bed[p.y*n+p.x].x;
        range=vec2(min(range.x,h),max(range.y,h));
    }}
    bounds[id.y*n+id.x]=range;
}
@compute @workgroup_size(8,8)
fn reduce(@builtin(global_invocation_id) id: vec3<u32>) {
    let side=params.x>>params.y;
    if any(id.xy>=vec2(side)) { return; }
    let child=offset(params.y-1u)+id.y*2u*(side*2u)+id.x*2u;
    let a=bounds[child]; let b=bounds[child+1u];
    let c=bounds[child+side*2u]; let d=bounds[child+side*2u+1u];
    bounds[offset(params.y)+id.y*side+id.x]=vec2(min(min(a.x,b.x),min(c.x,d.x)),max(max(a.y,b.y),max(c.y,d.y)));
}
