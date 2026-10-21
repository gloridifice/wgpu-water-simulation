// Hydrostatic reconstruction + local Lax-Friedrichs flux on a rectangular grid.
// State blocks: visible (foam,wetness,h,u), conservative (h,hu,hv,hc),
// ground (stranded foam,last wet concentration,eta,v).
@group(0) @binding(0) var<uniform> shore: ShoreParams;
@group(0) @binding(1) var<storage, read> old_shore: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> new_shore: array<vec4<f32>>;
fn world(q: vec2<i32>) -> vec2<f32> {
    return shore.domain.xy + (vec2<f32>(q) + vec2(0.5)) * shore.domain.zw / shore.options.z;
}
fn state(q: vec2<i32>) -> vec4<f32> {
    let n = i32(shore.options.z);
    let c = clamp(q, vec2(0), vec2(n-1));
    var v = old_shore[u32(n*n + c.y*n+c.x)];
    if q.x < 0 { v.y = -v.y; }
    if q.y < 0 || q.y >= n { v.z = -v.z; }
    return v;
}
fn bed(q: vec2<i32>) -> f32 {
    let n = i32(shore.options.z);
    return bottom_height(world(clamp(q, vec2(0), vec2(n-1))), shore);
}
fn velocity(q: vec4<f32>) -> vec2<f32> {
    return q.yz / max(q.x, 0.005);
}
fn physical(q: vec4<f32>, axis: u32) -> vec4<f32> {
    let v = velocity(q);
    let un = v[axis];
    var f = q * un;
    f[axis+1u] += 4.905*q.x*q.x;
    return f;
}
// Flux oriented outward from the cell, including its hydrostatic correction.
fn face(c: vec2<i32>, delta: vec2<i32>, axis: u32) -> vec4<f32> {
    let a = state(c); let b = state(c+delta);
    let za = bed(c); let zb = bed(c+delta);
    let top = max(za,zb);
    let ha = max(0.0,a.x+za-top); let hb = max(0.0,b.x+zb-top);
    let qa = vec4(ha, velocity(a)*ha, a.w/max(a.x,0.000001)*ha);
    let qb = vec4(hb, velocity(b)*hb, b.w/max(b.x,0.000001)*hb);
    let speed = max(abs(velocity(qa)[axis])+sqrt(9.81*ha),abs(velocity(qb)[axis])+sqrt(9.81*hb));
    let sign = f32(delta[axis]);
    var f = 0.5*(physical(qa,axis)+physical(qb,axis))*sign - 0.5*speed*(qb-qa);
    f[axis+1u] += sign*4.905*(a.x*a.x-ha*ha);
    return f;
}
// Same undeformed world-to-FFT coordinates as the ocean renderer, including half tile.
@group(0) @binding(3) var<storage, read> fft_now: array<vec2<f32>>;
@group(0) @binding(4) var<storage, read> fft_before: array<vec2<f32>>;
fn fft_at(q: vec2<i32>, channel: u32) -> f32 {
    let n=i32(shore.coupling.x); let w=((q%vec2(n))+vec2(n))%vec2(n);
    let index=channel*u32(n*n)+u32(w.y*n+w.x);
    return mix(fft_before[index].x,fft_now[3u*u32(n*n)+index].x,shore.coupling.w);
}
fn incident(p: vec2<f32>, depth: f32) -> vec3<f32> {
    let grid=(p/shore.coupling.y+vec2(0.5))*shore.coupling.x;
    let q=vec2<i32>(floor(grid)); let f=fract(grid);
    var result=vec3(0.0);
    for(var channel=0u;channel<3u;channel++) {
        result[channel]=mix(mix(fft_at(q,channel),fft_at(q+vec2(1,0),channel),f.x),
            mix(fft_at(q+vec2(0,1),channel),fft_at(q+vec2(1,1),channel),f.x),f.y);
    }
    result*=shore.coupling.z*smoothstep(0.0,6.0,shore.waves.y);
    // Preserve the elevation/momentum ratio when large external waves exceed
    // the local depth/velocity range of this linear boundary approximation.
    let safety=min(1.0,min(0.5*depth/max(abs(result.x),0.000001),
        8.0*depth/max(length(result.yz),0.000001)));
    return result*safety;
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = u32(shore.options.z); let cells=n*n;
    if id.x>=n || id.y>=n { return; }
    let index=id.y*n+id.x; let c=vec2<i32>(id.xy);
    let p=world(c); let z=bed(c); let dt=shore.timing.y;
    if !shore_active(shore) {
        new_shore[index]=vec4(0.0); new_shore[cells+index]=vec4(0.0); new_shore[2u*cells+index]=vec4(0.0); return;
    }
    if shore.options.w>0.5 {
        let h=max(-z,0.0);
        new_shore[index]=vec4(0.0,0.0,h,0.0);
        new_shore[cells+index]=vec4(h,0.0,0.0,0.0);
        new_shore[2u*cells+index]=vec4(0.0,0.0,max(z,0.0),0.0); return;
    }
    if dt==0.0 {
        new_shore[index]=old_shore[index]; new_shore[cells+index]=old_shore[cells+index];
        new_shore[2u*cells+index]=old_shore[2u*cells+index]; return;
    }
    let previous=state(c);
    let spacing=shore.domain.zw/f32(n);
    var q=previous-dt*((face(c,vec2(1,0),0u)+face(c,vec2(-1,0),0u))/spacing.x
        +(face(c,vec2(0,1),1u)+face(c,vec2(0,-1),1u))/spacing.y);
    // One-way FFT boundary relaxation; outgoing SWE disturbances are damped here.
    let edge=smoothstep(0.80,1.0,(f32(id.x)+0.5)/f32(n));
    let incoming=incident(p,max(-z,0.1));
    let imposed=vec4(max(incoming.x-z,0.0),incoming.yz,0.0);
    q=mix(q,imposed,1.0-exp(-dt*edge*4.0));
    q.x=max(q.x,0.0);
    if q.x<0.00001 { q=vec4(0.0); }
    let v=velocity(q);
    let friction=9.81*0.03*0.03*length(v)/pow(max(q.x,0.005),1.3333333);
    q.y=q.y/(1.0+dt*friction); q.z=q.z/(1.0+dt*friction);
    let speed=length(velocity(q));
    q.y=q.y*min(1.0,12.0/max(speed,0.00001)); q.z=q.z*min(1.0,12.0/max(speed,0.00001));
    let wave_speed=sqrt(9.81*max(q.x,0.001));
    let rising=max(0.0,(q.x-previous.x)/dt);
    let breaking=smoothstep(0.25,0.65,rising/wave_speed);
    let bore=smoothstep(1.0,1.4,length(velocity(q))/wave_speed);
    let swash=(1.0-smoothstep(0.08,0.25,q.x))*smoothstep(0.3,0.9,length(velocity(q)));
    let source=shore.options.x*(breaking+0.5*bore+swash)*(1.0-edge);
    q.w=clamp(q.w*exp(-dt*shore.timing.z)+dt*source*q.x,0.0,q.x);
    let old=old_shore[index]; let ground=old_shore[2u*cells+index];
    let wet=q.x>0.003;
    let concentration=clamp(q.w/max(q.x,0.00001),0.0,1.0);
    var stranded=ground.x*exp(-dt/10.0);
    if !wet && old.z>0.003 { stranded=max(stranded,ground.y); }
    if wet { stranded*=exp(-dt/0.12); }
    let wetness=select(old.y*exp(-dt/shore.timing.w),1.0,wet);
    new_shore[index]=vec4(concentration,wetness,q.x,velocity(q).x);
    new_shore[cells+index]=q;
    new_shore[2u*cells+index]=vec4(stranded,select(0.0,concentration,wet),q.x+z,velocity(q).y);
}

