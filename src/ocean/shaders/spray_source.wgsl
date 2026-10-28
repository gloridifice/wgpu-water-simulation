@group(0) @binding(0) var<uniform> spray: SprayParams;
@group(0) @binding(1) var<uniform> shore: ShoreParams;
@group(0) @binding(2) var<storage,read> current: array<vec4<f32>>;
@group(0) @binding(3) var<storage,read> previous: array<vec4<f32>>;
@group(0) @binding(4) var<storage,read> terrain_bed: array<vec2<f32>>;
@group(0) @binding(5) var<storage,read_write> signals: array<SpraySignal>;
fn elevation(c: vec2<i32>) -> f32 {
    let n=i32(shore.options.z); let q=clamp(c,vec2(0),vec2(n-1));
    return current[2u*u32(n*n)+u32(q.y*n+q.x)].z;
}
@compute @workgroup_size(64)
fn emit_source(@builtin(global_invocation_id) id: vec3<u32>) {
    let i=id.x; let n=u32(shore.options.z); let cells=n*n;
    if i>=cells || shore.timing.y<=0.0 || shore.options.w>0.5 { return; }
    let c=vec2<i32>(i32(i%n),i32(i/n));
    let spacing=shore.domain.zw/f32(n);
    let pos=shore.domain.xy+(vec2<f32>(c)+vec2(0.5))*spacing;
    // Only the pure SWE region: no approximate inversion of displaced FFT geometry.
    if ocean_weight(pos,shore)>0.001 || any(c<vec2(1)) || any(c>=vec2(i32(n)-1)) { return; }
    let q=current[cells+i]; let old=previous[cells+i];
    if q.x<=spray.emission.w { return; }
    let gradient=vec2(elevation(c+vec2(1,0))-elevation(c-vec2(1,0)),
        elevation(c+vec2(0,1))-elevation(c-vec2(0,1)))/(2.0*spacing);
    let rise=max(0.0,(q.x-old.x)/shore.timing.y);
    let weight=smoothstep(spray.emission.y,spray.emission.y+0.1,length(gradient))
        *smoothstep(spray.emission.z,spray.emission.z+0.2,rise)
        *smoothstep(spray.emission.w,2.0*spray.emission.w,q.x);
    let amount=spray.emission.x*spacing.x*spacing.y*shore.timing.y*weight;
    let water=q.yz/max(q.x,0.00001);
    let direction=water/max(length(water),0.02);
    let v=water+direction*spray.motion.y;
    let position=vec3(pos.x,current[2u*cells+i].z,pos.y);
    signals[i].position_amount+=vec4(position*amount,amount);
    signals[i].velocity_reserved+=vec4(vec3(v.x,spray.motion.x,v.y)*amount,0.0);
}
