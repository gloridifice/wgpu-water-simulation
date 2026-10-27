@group(0) @binding(0) var<uniform> spray: SprayParams;
@group(0) @binding(1) var<uniform> shore: ShoreParams;
@group(0) @binding(2) var<storage,read> water: array<vec4<f32>>;
@group(0) @binding(4) var<storage,read> terrain_bed: array<vec2<f32>>;
@group(0) @binding(5) var<storage,read> signals: array<SpraySignal>;
@group(0) @binding(6) var<storage,read> old_particles: ParticleList;
@group(0) @binding(7) var<storage,read_write> particles: ParticleOutput;
struct EmissionState { fraction: f32, sequence: u32 }
@group(0) @binding(8) var<storage,read_write> emission: array<EmissionState>;
@group(0) @binding(9) var<storage,read_write> indirect: array<u32>;
fn hash(value:u32) -> u32 {
    var x=value; x=(x^(x>>16u))*0x7feb352du; x=(x^(x>>15u))*0x846ca68bu; return x^(x>>16u);
}
fn random(value:u32) -> f32 { return f32(hash(value)&0x00ffffffu)/16777216.0; }
fn inside(p:vec2<f32>) -> bool {
    let margin=shore.domain.zw/shore.options.z;
    return all(p>=shore.domain.xy+margin) && all(p<shore.domain.xy+shore.domain.zw-margin)
        && ocean_weight(p,shore)<0.001;
}
// Bilinear wet-only elevation, matching nearshore rendering around dry obstacles.
// Returned y is interpolated depth. Visual foam lift never changes particle motion.
fn surface(p:vec2<f32>) -> vec2<f32> {
    let n=u32(shore.options.z); let cells=n*n;
    let grid=(p-shore.domain.xy)/shore.domain.zw*f32(n)-vec2(0.5);
    let c=vec2<i32>(floor(grid)); let f=fract(grid);
    var height=0.0; var wet=0.0; var depth=0.0;
    for(var y=0;y<2;y++) { for(var x=0;x<2;x++) {
        let q=clamp(c+vec2(x,y),vec2(0),vec2(i32(n)-1)); let i=u32(q.y)*n+u32(q.x);
        let w=select(1.0-f.x,f.x,x==1)*select(1.0-f.y,f.y,y==1);
        let h=water[cells+i].x; depth+=w*h;
        if h>0.00001 { height+=w*water[2u*cells+i].z; wet+=w; }
    }}
    let bed=bottom_height(p,shore);
    let water_height=select(bed,height/max(wet,0.000001),wet>0.000001);
    return vec2(max(water_height,bed),depth);
}
@compute @workgroup_size(64)
fn update_particles(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=min(old_particles.count,spray.counts.x) { return; }
    var p=old_particles.items[id.x];
    p.position_age.w+=spray.style.z;
    if p.position_age.w>=p.velocity_life.w { return; }
    let steps=u32(clamp(ceil(spray.style.z/0.005),1.0,32.0));
    let dt=spray.style.z/f32(steps);
    for(var j=0u;j<steps;j++) {
        let decay=exp(-spray.style.x*dt);
        let k=select(dt,(1.0-decay)/max(spray.style.x,0.00001),spray.style.x>0.00001);
        let gravity=vec3(0.0,-9.81,0.0);
        // Stable exponential drag; bounded substeps for surface crossing.
        p.velocity_life=vec4(p.velocity_life.xyz*decay+gravity*k,p.velocity_life.w);
        p.position_age=vec4(p.position_age.xyz+p.velocity_life.xyz*dt,p.position_age.w);
        if !all(abs(p.position_age.xyz)<vec3(1000000.0)) || !inside(p.position_age.xz) { return; }
        if p.position_age.y<=surface(p.position_age.xz).x { return; }
    }
    let slot=atomicAdd(&particles.count,1u);
    if slot<spray.counts.x { particles.items[slot]=p; }
}
@compute @workgroup_size(64)
fn spawn_particles(@builtin(global_invocation_id) id:vec3<u32>) {
    let i=id.x; if i>=spray.counts.y { return; }
    let s=signals[i]; let amount=s.position_amount.w;
    if !(amount>0.0) { return; }
    let total=emission[i].fraction+amount;
    let requested=u32(min(floor(total),100000000.0));
    emission[i].fraction=fract(total);
    let sequence=emission[i].sequence; emission[i].sequence+=requested;
    let pos=s.position_amount.xyz/amount; let velocity=s.velocity_reserved.xyz/amount;
    if !inside(pos.xz) || surface(pos.xz).y<=spray.emission.w { return; }
    // Ticket allocation is bounded by the pool, not by arbitrarily large source rates.
    for(var j=0u;j<min(requested,spray.counts.x);j++) {
        let seed=hash(i^spray.counts.z)^hash(sequence+j);
        let r=vec3(random(seed),random(seed+1u),random(seed+2u));
        let horizontal=pos.xz+(r.xz-vec2(0.5))*shore.domain.zw/shore.options.z*0.8;
        if !inside(horizontal) { continue; }
        let water_surface=surface(horizontal);
        if water_surface.y<=spray.emission.w { continue; }
        // At most one failed ticket per source cell; count cannot wrap at supported resolutions.
        let slot=atomicAdd(&particles.count,1u);
        if slot>=spray.counts.x { return; }
        let start=vec3(horizontal.x,water_surface.x+0.025,horizontal.y);
        let v=velocity+vec3((r.x-0.5)*0.8,r.y*0.5,(r.z-0.5)*0.8);
        particles.items[slot]=Particle(vec4(start,0.0),vec4(v,spray.motion.z*(0.75+0.25*r.z)),
            vec4(spray.motion.w*mix(spray.grid.z,spray.grid.w,r.x),r.y*6.2831853,floor(random(seed+3u)*4.0),0.0));
    }
}
@compute @workgroup_size(64)
fn finish_particles(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x!=0u { return; }
    atomicMin(&particles.count,spray.counts.x);
    indirect[0]=6u; indirect[1]=atomicLoad(&particles.count); indirect[2]=0u; indirect[3]=0u;
}
