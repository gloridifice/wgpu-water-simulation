// Hydrostatic reconstruction + local Lax-Friedrichs flux on a rectangular grid.
// State blocks: visible (surface foam density,wetness,h,u),
// conservative (h,hu,hv,surface foam amount per area), ground (deposit,last wet density,eta,v).
@group(0) @binding(0) var<uniform> shore: ShoreParams;
@group(0) @binding(1) var<storage, read> old_shore: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> new_shore: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read> initial_shore: array<vec4<f32>>;
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
// Shared face velocity: identical on either side, zero at closed boundaries.
// Dry cells may receive foam from advancing water but cannot export it.
fn foam_velocity(c: vec2<i32>, delta: vec2<i32>, axis: u32) -> f32 {
    let b=c+delta; let n=i32(shore.options.z);
    if any(b<vec2(0)) || any(b>=vec2(n)) { return 0.0; }
    let a_state=state(c); let b_state=state(b);
    let speed=0.5*(velocity(a_state)[axis]+velocity(b_state)[axis])*f32(delta[axis]);
    let donor=select(b_state,a_state,speed>=0.0);
    return select(0.0,speed,donor.x>shore.density.z);
}
fn foam_flux(c: vec2<i32>, delta: vec2<i32>, axis: u32) -> f32 {
    let speed=foam_velocity(c,delta,axis);
    return speed*select(state(c+delta).w,state(c).w,speed>=0.0);
}
fn physical(q: vec4<f32>, axis: u32) -> vec4<f32> {
    let v = velocity(q);
    let un = v[axis];
    var f = q * un;
    f[axis+1u] += 4.905*q.x*q.x;
    return f;
}
// Keep the original first-order flux as the reference path.
fn first_order_face(c: vec2<i32>, delta: vec2<i32>, axis: u32) -> vec4<f32> {
    let a=state(c); let b=state(c+delta);
    let za=bed(c); let zb=bed(c+delta); let top=max(za,zb);
    let ha=max(0.0,a.x+za-top); let hb=max(0.0,b.x+zb-top);
    let qa=vec4(ha,velocity(a)*ha,0.0);
    let qb=vec4(hb,velocity(b)*hb,0.0);
    let speed=max(abs(velocity(qa)[axis])+sqrt(9.81*ha),abs(velocity(qb)[axis])+sqrt(9.81*hb));
    let sign=f32(delta[axis]);
    var f=0.5*(physical(qa,axis)+physical(qb,axis))*sign-0.5*speed*(qb-qa);
    f[axis+1u]+=sign*4.905*(a.x*a.x-ha*ha);
    return f;
}
// MUSCL reconstruction of free surface, velocity and bed; foam has its own flux.
// Revert locally at wet/dry fronts and domain edges; keep both face depths positive.
fn primitive(c: vec2<i32>) -> vec4<f32> {
    let q=state(c);
    return vec4(q.x+bed(c), velocity(q), 0.0);
}
fn limited(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return select(vec4(0.0), sign(a)*min(min(1.3*abs(a),1.3*abs(b)),0.5*abs(a+b)), a*b>vec4(0.0));
}
struct Reconstruction { value: vec4<f32>, slope: vec4<f32>, bottom: f32, bed_slope: f32 }
fn reconstruct(c: vec2<i32>, axis: u32) -> Reconstruction {
    let n=i32(shore.options.z);
    let v=primitive(c); let z=bed(c);
    var r=Reconstruction(v,vec4(0.0),z,0.0);
    if shore.solver.x<1.5 || any(c<=vec2(0)) || any(c>=vec2(n-1)) { return r; }
    var d=vec2(0); d[axis]=1;
    if min(state(c).x,min(state(c-d).x,state(c+d).x))<0.02 { return r; }
    r.slope=limited(v-primitive(c-d),primitive(c+d)-v);
    r.bed_slope=limited(vec4(z-bed(c-d)),vec4(bed(c+d)-z)).x;
    let dh=r.slope.x-r.bed_slope;
    if state(c).x-0.5*abs(dh)<0.005 { r.slope=vec4(0.0); r.bed_slope=0.0; }
    return r;
}
// Outward hydrostatic flux with the one-sided pressure correction.
fn face(c: vec2<i32>, delta: vec2<i32>, axis: u32) -> vec4<f32> {
    if shore.solver.x<1.5 { return first_order_face(c,delta,axis); }
    let a=reconstruct(c,axis); let b=reconstruct(c+delta,axis);
    let sign=f32(delta[axis]);
    let va=a.value+0.5*sign*a.slope; let vb=b.value-0.5*sign*b.slope;
    let za=a.bottom+0.5*sign*a.bed_slope; let zb=b.bottom-0.5*sign*b.bed_slope;
    let top=max(za,zb);
    let raw_h=max(va.x-za,0.0);
    let ha=max(va.x-top,0.0); let hb=max(vb.x-top,0.0);
    let qa=vec4(ha,va.yz*ha,0.0);
    let qb=vec4(hb,vb.yz*hb,0.0);
    let speed=max(abs(velocity(qa)[axis])+sqrt(9.81*ha),abs(velocity(qb)[axis])+sqrt(9.81*hb));
    var f=0.5*(physical(qa,axis)+physical(qb,axis))*sign-0.5*speed*(qb-qa);
    f[axis+1u]+=sign*4.905*(raw_h*raw_h-ha*ha);
    return f;
}
fn bed_source(c: vec2<i32>, axis: u32) -> f32 {
    if shore.solver.x<1.5 {return 0.0;}
    let r=reconstruct(c,axis);
    let hl=max(r.value.x-0.5*r.slope.x-r.bottom+0.5*r.bed_slope,0.0);
    let hr=max(r.value.x+0.5*r.slope.x-r.bottom-0.5*r.bed_slope,0.0);
    return -4.905*(hl+hr)*r.bed_slope;
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
    result*=shore.coupling.z;
    // Preserve the elevation/momentum ratio when large external waves exceed
    // the local depth/velocity range of this linear boundary approximation.
    let safety=min(1.0,min(0.5*depth/max(abs(result.x),0.000001),
        8.0*depth/max(length(result.yz),0.000001)));
    return result*safety;
}
fn foam_breaking_weight(rise_ratio: f32) -> f32 {
    let threshold=shore.foam_generation.x;
    let width=shore.foam_generation.y;
    // Strict comparison keeps still water quiet even with both controls at zero.
    if width<=0.0 { return select(0.0,1.0,rise_ratio>threshold); }
    return smoothstep(threshold,threshold+width,rise_ratio);
}
// Sample one immutable solver stage; never compare a newly updated center with
// old neighbors. World-space direction accounts for unequal X/Z cell spacing.
fn foam_surface_cell(c: vec2<i32>) -> vec2<f32> {
    let depth=state(c).x;
    // Dry terrain has no free surface and must not contribute a fake height.
    let wet=select(0.0,1.0,depth>0.00001);
    return vec2((bed(c)+depth)*wet,wet);
}
fn foam_surface_at(grid: vec2<f32>) -> vec2<f32> {
    let c=vec2<i32>(floor(grid)); let f=fract(grid);
    // Interpolate height only over wet contributors, preserving a level surface
    // beside dry cells. Return weighted height and coverage for normalization.
    return mix(mix(foam_surface_cell(c),foam_surface_cell(c+vec2(1,0)),f.x),
        mix(foam_surface_cell(c+vec2(0,1)),foam_surface_cell(c+vec2(1,1)),f.x),f.y);
}
fn foam_front_gain(value: f32) -> f32 {
    let p=shore.foam_front_gain;
    var rise=select(0.0,1.0,value>=p.y);
    var fall=select(0.0,1.0,value<=p.z);
    // Coincident control points mean a hard edge, never smoothstep(a,a,x).
    if p.y>p.x { rise=smoothstep(p.x,p.y,value); }
    if p.w>p.z { fall=1.0-smoothstep(p.z,p.w,value); }
    return rise*fall;
}
fn foam_front_weight(c: vec2<i32>) -> f32 {
    if shore.foam_front.x<=0.0 { return 0.0; }
    let center=state(c); let flow=velocity(center); let speed=length(flow);
    // A stationary surface has no forward direction; suppress tiny velocity noise.
    if speed<=0.02 || center.x<=0.00001 { return 0.0; }
    let surface=bed(c)+center.x;
    let spacing=shore.domain.zw/shore.options.z;
    let offset=flow/speed*shore.foam_front.y/spacing;
    var drop=0.0;
    for(var sample=1u;sample<=4u;sample++) {
        let ahead=vec2<f32>(c)+offset*(f32(sample)/4.0);
        // Leaving the patch is not evidence of a falling free surface.
        if any(ahead<vec2(0.0)) || any(ahead>vec2(shore.options.z-1.0)) { continue; }
        let sampled=foam_surface_at(ahead);
        if sampled.y<=0.000001 { continue; }
        drop=max(drop,surface-sampled.x/sampled.y);
    }
    // Deliberately no dry-front, rising-water or shore-direction requirement.
    let fall_weight=smoothstep(0.0,shore.foam_front.z,drop);
    return fall_weight*foam_front_gain(fall_weight)*smoothstep(0.02,0.1,speed);
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
        // Seed the entire patch from the CURRENT filtered FFT, not old history.
        // Dry land stays motionless; shallow cells retain the velocity safeguard.
        let incoming=incident(p,max(-z,0.1));
        let h=max(incoming.x-z,0.0);
        var momentum=incoming.yz;
        momentum*=min(1.0,8.0*h/max(length(momentum),0.000001));
        if h<=0.003 { momentum=vec2(0.0); }
        let v=momentum/max(h,0.005);
        new_shore[index]=vec4(0.0,0.0,h,v.x);
        new_shore[cells+index]=vec4(h,momentum,0.0);
        new_shore[2u*cells+index]=vec4(0.0,0.0,h+z,v.y); return;
    }
    if dt==0.0 {
        new_shore[index]=old_shore[index]; new_shore[cells+index]=old_shore[cells+index];
        new_shore[2u*cells+index]=old_shore[2u*cells+index]; return;
    }
    let previous=state(c);
    let original=initial_shore[cells+index];
    let spacing=shore.domain.zw/f32(n);
    var q=previous-dt*((face(c,vec2(1,0),0u)+face(c,vec2(-1,0),0u))/spacing.x
        +(face(c,vec2(0,1),1u)+face(c,vec2(0,-1),1u))/spacing.y);
    // Conservative surface transport, independent of water depth. Convergence
    // increases density automatically; adding a second compression source would
    // double count it. Upwind flux stays positive under the existing SWE CFL.
    q.w=max(0.0,previous.w-dt*((foam_flux(c,vec2(1,0),0u)+foam_flux(c,vec2(-1,0),0u))/spacing.x
        +(foam_flux(c,vec2(0,1),1u)+foam_flux(c,vec2(0,-1),1u))/spacing.y));
    q.y+=dt*bed_source(c,0u)/spacing.x; q.z+=dt*bed_source(c,1u)/spacing.y;
    // One-way FFT boundary relaxation; outgoing SWE disturbances are damped here.
    let edge=smoothstep(0.80,1.0,(f32(id.x)+0.5)/f32(n));
    let incoming=incident(p,max(-z,0.1));
    let imposed=vec4(max(incoming.x-z,0.0),incoming.yz,0.0);
    if shore.solver.x>1.5 {
        q+=dt*edge*4.0*(imposed-previous);
    } else { q=mix(q,imposed,1.0-exp(-dt*edge*4.0)); }
    q.x=max(q.x,0.0);
    if q.x<0.00001 { q=vec4(0.0,0.0,0.0,q.w); }
    let v=velocity(q);
    let friction=9.81*0.03*0.03*length(v)/pow(max(q.x,0.005),1.3333333);
    if shore.solver.x>1.5 {
        let f=9.81*0.03*0.03*length(velocity(previous))/pow(max(previous.x,0.005),1.3333333);
        q.y-=min(dt*f,1.0)*previous.y; q.z-=min(dt*f,1.0)*previous.z;
        if shore.solver.y>1.5 { q=0.5*(original+q); }
    } else { q.y=q.y/(1.0+dt*friction); q.z=q.z/(1.0+dt*friction); }
    q.x=max(q.x,0.0);
    if q.x<0.00001 { q=vec4(0.0,0.0,0.0,q.w); }
    let speed=length(velocity(q));
    q.y=q.y*min(1.0,12.0/max(speed,0.00001)); q.z=q.z*min(1.0,12.0/max(speed,0.00001));
    if shore.solver.y>0.5 && shore.solver.y<1.5 {
        new_shore[index]=initial_shore[index]; new_shore[cells+index]=q;
        new_shore[2u*cells+index]=initial_shore[2u*cells+index]; return;
    }
    let wave_speed=sqrt(9.81*max(q.x,0.001));
    let rising=max(0.0,(q.x-original.x)/dt);
    let breaking=foam_breaking_weight(rising/wave_speed);
    let bore=smoothstep(1.0,1.4,length(velocity(q))/wave_speed);
    let divergence=(foam_velocity(c,vec2(1,0),0u)+foam_velocity(c,vec2(-1,0),0u))/spacing.x
        +(foam_velocity(c,vec2(0,1),1u)+foam_velocity(c,vec2(0,-1),1u))/spacing.y;
    // SWE cannot resolve overturning/air entrainment. Rising water plus a
    // compressive bore is a tunable proxy; shallow fast retreat alone emits none.
    let compression=smoothstep(shore.density.x,shore.density.x+0.5,max(0.0,-divergence));
    let background_source=shore.options.x*shore.foam_front.w*breaking*mix(0.25,1.0,max(bore,compression));
    let front_source=shore.foam_front.x*foam_front_weight(c);
    let source=(background_source+front_source)
        *smoothstep(0.003,0.03,q.x)*(1.0-edge);
    let attenuation=exp(-dt*shore.timing.z);
    let source_time=select(dt,(1.0-attenuation)/max(shore.timing.z,0.000001),shore.timing.z>0.00001);
    q.w=max(0.0,q.w*attenuation+source*source_time);
    let old=initial_shore[index]; let ground=initial_shore[2u*cells+index];
    let wet=q.x>shore.density.z;
    let surface_density=q.w;
    var stranded=ground.x*exp(-dt/10.0);
    if !wet { stranded+=q.w; q.w=0.0; }
    if wet { stranded*=exp(-dt/0.12); }
    let wetness=select(old.y*exp(-dt/shore.timing.w),1.0,q.x>0.003);
    new_shore[index]=vec4(q.w,wetness,q.x,velocity(q).x);
    new_shore[cells+index]=q;
    new_shore[2u*cells+index]=vec4(stranded,select(0.0,surface_density,wet),q.x+z,velocity(q).y);
}

