// Hydrostatic reconstruction + local Lax-Friedrichs flux on a rectangular grid.
// State blocks: visible (surface foam density,wetness,h,u),
// conservative (h,hu,hv,surface foam amount per area), surface (source rate,front weight,eta,v).
// Diagnostic x/y are populated only for ridge mode or source/weight debug views.
// Fourth block: foam area momentum X/Z and texture A displacement X/Z in meters.
// Fifth block: texture B displacement X/Z, shared cycle phase, waterborne R.
// In dual mode q.w is N+R (for shared height/debug consumers); N=q.w-R.
// Legacy mode keeps R=0. Neither concentration is clipped to 1.
// Zero keeps uniform-driven diagnostic entries; production specializes the
// three supported combinations so inactive high-order paths cannot inflate
// register pressure or retain terrain work in a cheaper solver variant.
override SWE_ORDER: u32 = 0u;
override FOAM_ORDER: u32 = 0u;
fn second_order_swe() -> bool {
    if SWE_ORDER != 0u { return SWE_ORDER == 2u; }
    return shore.solver.x>1.5;
}
fn second_order_foam() -> bool {
    if FOAM_ORDER != 0u { return FOAM_ORDER == 2u; }
    return shore.foam_inertia.z>1.5;
}
@group(0) @binding(0) var<uniform> shore: ShoreParams;
@group(0) @binding(1) var<storage, read> old_shore: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> new_shore: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read> initial_shore: array<vec4<f32>>;
@group(0) @binding(7) var<storage, read> terrain_bed: array<vec2<f32>>;
fn world(q: vec2<i32>) -> vec2<f32> {
    return shore.domain.xy + (vec2<f32>(q) + vec2(0.5)) * shore.domain.zw / shore.options.z;
}
fn state(q: vec2<i32>) -> vec4<f32> {
    let n = i32(shore.options.z);
    let c = clamp(q, vec2(0), vec2(n-1));
    var v = old_shore[u32(n*n + c.y*n+c.x)];
    if shore.model.x>0.5 {
        // Zero-gradient open ghosts, reflective closed ghosts. Incoming waves
        // are supplied by the FFT relaxation band, never a second wavemaker.
        if (q.x<0 && shore.edges.x<0.5) || (q.x>=n && shore.edges.y<0.5) { v.y=-v.y; }
        if (q.y<0 && shore.edges.z<0.5) || (q.y>=n && shore.edges.w<0.5) { v.z=-v.z; }
    } else {
        if q.x < 0 { v.y = -v.y; }
        if q.y < 0 || q.y >= n { v.z = -v.z; }
    }
    return v;
}
fn model_open_outside(q: vec2<i32>) -> bool {
    let n=i32(shore.options.z);
    return shore.model.x>0.5 && ((q.x<0 && shore.edges.x>0.5) || (q.x>=n && shore.edges.y>0.5)
        || (q.y<0 && shore.edges.z>0.5) || (q.y>=n && shore.edges.w>0.5));
}
// The uploaded model bed already contains solver cell-center samples.
// Only arbitrary world-space consumers need model_terrain's interpolation.
fn cell_bottom(q: vec2<i32>, s: ShoreParams) -> f32 {
    let n=i32(s.options.z); let c=clamp(q,vec2(0),vec2(n-1));
    let p=s.domain.xy+(vec2<f32>(c)+vec2(0.5))*s.domain.zw/s.options.z;
    var base: f32;
    if s.model.x>0.5 { base=terrain_cell(c,s).x; }
    else { base=terrain_height(p,s); }
    return collider_bottom(p,base,s);
}
fn bed(q: vec2<i32>) -> f32 {
    return cell_bottom(q,shore);
}
fn velocity(q: vec4<f32>) -> vec2<f32> {
    return q.yz / max(q.x, 0.005);
}
// Foam must accumulate in front of an emergent solid instead of leaking
// through its side. Wet overtopped cells remain ordinary transport cells.
fn collider_blocks_face(a: vec2<i32>, b: vec2<i32>) -> bool {
    if shore.collider_meta.x<0.5 && shore.model.x<0.5 { return false; }
    let za=bed(a); let zb=bed(b);
    let raised=shore.model.x>0.5 || za>terrain_height(world(a),shore)+0.00001 || zb>terrain_height(world(b),shore)+0.00001;
    return raised && max(za,zb)>=max(za+state(a).x,zb+state(b).x);
}
// Shared face velocity: identical on either side, zero at closed boundaries.
// Dry cells may receive foam from advancing water but cannot export it.
fn water_face_velocity(c: vec2<i32>, delta: vec2<i32>, axis: u32) -> f32 {
    let b=c+delta; let n=i32(shore.options.z);
    if any(b<vec2(0)) || any(b>=vec2(n)) {
        if model_open_outside(b) { return velocity(state(c))[axis]*f32(delta[axis]); }
        return 0.0;
    }
    if collider_blocks_face(c,b) { return 0.0; }
    let a_state=state(c); let b_state=state(b);
    let speed=0.5*(velocity(a_state)[axis]+velocity(b_state)[axis])*f32(delta[axis]);
    let donor=select(b_state,a_state,speed>=0.0);
    return select(0.0,speed,donor.x>shore.density.z);
}
fn foam_response(previous: vec2<f32>, water: vec2<f32>, dt: f32) -> vec2<f32> {
    if shore.foam_inertia.x<0.5 { return previous; }
    let desired_velocity=water*shore.foam_motion.x;
    if shore.foam_motion.y<=0.0 { return desired_velocity; }
    return mix(previous,desired_velocity,1.0-exp(-dt/shore.foam_motion.y));
}
fn foam_speed(amount: f32, momentum: vec2<f32>, water: vec2<f32>) -> vec2<f32> {
    // Empty cells carry no momentum; water supplies only a texture velocity.
    if amount<=0.0 { return water*shore.foam_motion.x; }
    let speed=momentum/amount;
    return speed*min(1.0,12.0/max(length(speed),0.000001));
}
fn foam_state(c: vec2<i32>) -> vec3<f32> {
    let n=i32(shore.options.z); let p=clamp(c,vec2(0),vec2(n-1));
    let q=state(c);
    return vec3(q.w,old_shore[u32(3*n*n+p.y*n+p.x)].xy);
}
fn foam_transport_velocity(c: vec2<i32>) -> vec2<f32> {
    if shore.foam_lifetime.x>0.5 { return velocity(state(c)); }
    let foam=foam_state(c); let water=velocity(state(c));
    // Midpoint response drives both conservative density flux and texture motion.
    return foam_response(foam_speed(foam.x,foam.yz,water),water,shore.timing.y*0.5);
}
fn initial_motion_at(c: vec2<i32>) -> vec4<f32> {
    let n=i32(shore.options.z);
    if any(c<vec2(0)) || any(c>=vec2(n)) { return vec4(0.0); }
    return initial_shore[u32(3*n*n+c.y*n+c.x)];
}
// Interpolate coordinates only across wet donors. Blending against a cleared
// dry-cell displacement would manufacture shear right at the waterline.
fn sample_initial_texture(grid: vec2<f32>) -> vec4<f32> {
    let n=i32(shore.options.z); let base=vec2<i32>(floor(grid)); let f=fract(grid);
    var value=vec4(0.0); var weight=0.0;
    for (var y=0; y<2; y++) { for (var x=0; x<2; x++) {
        let c=base+vec2(x,y);
        if any(c<vec2(0)) || any(c>=vec2(n)) { continue; }
        let i=u32(c.y*n+c.x); let cells=u32(n*n);
        if initial_shore[cells+i].x<=shore.density.z { continue; }
        let w=select(1.0-f.x,f.x,x==1)*select(1.0-f.y,f.y,y==1);
        value+=w*vec4(initial_shore[3u*cells+i].zw,initial_shore[4u*cells+i].xy);
        weight+=w;
    }}
    return value/max(weight,0.000001);
}
struct FoamMotionStep { motion: vec4<f32>, texture: vec4<f32> }
fn advance_foam_motion(c: vec2<i32>, q: vec4<f32>, original: vec4<f32>, momentum: vec2<f32>) -> FoamMotionStep {
    let dt=shore.timing.y;
    let spacing=shore.domain.zw/shore.options.z;
    var transport=foam_transport_velocity(c);
    if shore.solver.y>1.5 {
        let initial_speed=foam_speed(original.w,initial_motion_at(c).xy,velocity(original));
        transport=0.5*(transport+foam_response(initial_speed,velocity(original),dt*0.5));
    }
    var grid=vec2<f32>(c)-transport*dt/spacing;
    if shore.foam_lifetime.x>0.5 {
        transport=dual_transport_velocity(c);
        grid=dual_backtrace(c,transport);
        transport=(vec2<f32>(c)-grid)*spacing/dt;
    }
    let coords=sample_initial_texture(grid);
    let n=i32(shore.options.z);
    // Advance with actual simulation dt, not wall time (pause/stalls/RK2).
    let phase=initial_shore[u32(4*n*n+c.y*n+c.x)].z;
    let period=shore.foam_shading.w;
    var next=phase;
    var a=coords.xy+transport*dt;
    var b=coords.zw+transport*dt;
    if period>0.0 {
        let phase_step=dt/period;
        next=fract(phase+phase_step);
        // Reset only the invisible layer, retaining motion after the wrap instant.
        if phase+phase_step>=1.0 { a=transport*(next*period); }
        if fract(phase+0.5)+phase_step>=1.0 { b=transport*(fract(next+0.5)*period); }
    }
    let wet=q.x>shore.density.z;
    return FoamMotionStep(vec4(select(vec2(0.0),momentum,wet),select(vec2(0.0),a,wet)),
        vec4(select(vec2(0.0),b,wet),next,0.0));
}
// Conservative MUSCL states: one common slope scale keeps density positive
// AND bounds momentum/density by neighboring velocities, including near vacuum.
// The opposite faces average back to the cell state; never clip density to 1.
fn limit_foam_slope(left: vec3<f32>, q: vec3<f32>, right: vec3<f32>) -> vec3<f32> {
    let a=q-left; let b=right-q;
    var slope=select(vec3(0.0),sign(a)*min(min(2.0*abs(a),2.0*abs(b)),0.5*abs(a+b)),a*b>vec3(0.0));
    let v=q.yz/q.x;
    // Empty neighbors impose no velocity. A roundoff-sized margin avoids
    // cancelling a valid density slope in a constant non-binary velocity field.
    let vl=select(v,left.yz/max(left.x,1e-30),left.x>0.0);
    let vr=select(v,right.yz/max(right.x,1e-30),right.x>0.0);
    let tolerance=2e-6*max(vec2(1.0),abs(v));
    let lo=min(v,min(vl,vr))-tolerance; let hi=max(v,max(vl,vr))+tolerance;
    var scale=min(1.0,2.0*q.x/max(abs(slope.x),1e-30));
    let lower=max(vec2(0.0),q.yz-lo*q.x);
    let upper=max(vec2(0.0),hi*q.x-q.yz);
    let lower_scale=2.0*lower/max(abs(slope.yz-lo*slope.x),vec2(1e-30));
    let upper_scale=2.0*upper/max(abs(hi*slope.x-slope.yz),vec2(1e-30));
    scale=min(scale,min(min(lower_scale.x,lower_scale.y),min(upper_scale.x,upper_scale.y)));
    return slope*scale;
}
// Reuse a five-cell stencil per axis rather than reconstructing both sides of
// all four faces independently. Bed/collider queries are especially expensive
// on uploaded terrain; each stencil cell's geometry is evaluated just once.
struct FoamCell {
    position: vec2<i32>,
    water: vec4<f32>,
    foam: vec3<f32>,
    bottom: f32,
    raised: bool,
}
fn foam_cell(c: vec2<i32>) -> FoamCell {
    let n=i32(shore.options.z); let p=clamp(c,vec2(0),vec2(n-1));
    let water=state(c);
    var cell=FoamCell(c,water,vec3(water.w,old_shore[u32(3*n*n+p.y*n+p.x)].xy),0.0,false);
    if all(c>=vec2(0)) && all(c<vec2(n)) && (shore.model.x>0.5 || shore.collider_meta.x>0.5) {
        cell.bottom=bed(c);
        cell.raised=shore.model.x>0.5 || cell.bottom>terrain_height(world(c),shore)+0.00001;
    }
    return cell;
}
fn foam_blocked(a: FoamCell, b: FoamCell) -> bool {
    return (a.raised || b.raised) && max(a.bottom,b.bottom)>=max(a.bottom+a.water.x,b.bottom+b.water.x);
}
fn foam_slope(left: FoamCell, q: FoamCell, right: FoamCell, blocked_left: bool, blocked_right: bool) -> vec3<f32> {
    let n=i32(shore.options.z);
    if q.foam.x<=0.0 || any(q.position<=vec2(0)) || any(q.position>=vec2(n-1))
        || min(q.water.x,min(left.water.x,right.water.x))<=max(0.02,shore.density.z)
        || blocked_left || blocked_right { return vec3(0.0); }
    return limit_foam_slope(left.foam,q.foam,right.foam);
}
// Globally positive-axis flux: one shared face value, including open outflow.
fn foam_reconstructed_flux(a: FoamCell, b: FoamCell, qa: vec3<f32>, qb: vec3<f32>, blocked: bool, axis: u32) -> vec3<f32> {
    let n=i32(shore.options.z);
    if any(a.position<vec2(0)) {
        if model_open_outside(a.position) {
            let v=foam_response(foam_speed(b.foam.x,b.foam.yz,velocity(b.water)),velocity(b.water),shore.timing.y*0.5);
            return min(0.0,v[axis])*b.foam;
        }
        return vec3(0.0);
    }
    if any(b.position>=vec2(n)) {
        if model_open_outside(b.position) {
            let v=foam_response(foam_speed(a.foam.x,a.foam.yz,velocity(a.water)),velocity(a.water),shore.timing.y*0.5);
            return max(0.0,v[axis])*a.foam;
        }
        return vec3(0.0);
    }
    if blocked { return vec3(0.0); }
    let av=foam_response(foam_speed(qa.x,qa.yz,velocity(a.water)),velocity(a.water),shore.timing.y*0.5)[axis];
    let bv=foam_response(foam_speed(qb.x,qb.yz,velocity(b.water)),velocity(b.water),shore.timing.y*0.5)[axis];
    let signal=max(abs(av),abs(bv));
    let outgoing=select(0.0,0.5*(av+signal),a.water.x>shore.density.z);
    let incoming=select(0.0,0.5*(bv-signal),b.water.x>shore.density.z);
    return outgoing*qa+incoming*qb;
}
fn foam_axis_flux(center: FoamCell, axis: u32) -> vec3<f32> {
    var d=vec2(0); d[axis]=1;
    let a=foam_cell(center.position-d); let b=foam_cell(center.position+d);
    let aa=foam_cell(center.position-2*d); let bb=foam_cell(center.position+2*d);
    let left_block=foam_blocked(a,center); let right_block=foam_blocked(center,b);
    let middle=foam_slope(a,center,b,left_block,right_block);
    let left=foam_slope(aa,a,center,foam_blocked(aa,a),left_block);
    let right=foam_slope(center,b,bb,right_block,foam_blocked(b,bb));
    let positive=foam_reconstructed_flux(center,b,center.foam+0.5*middle,b.foam-0.5*right,right_block,axis);
    let negative=foam_reconstructed_flux(a,center,a.foam+0.5*left,center.foam-0.5*middle,left_block,axis);
    return positive-negative;
}
// Original first-order path stays available without the MUSCL stencil work.
fn foam_flux(c: vec2<i32>, delta: vec2<i32>, axis: u32) -> vec3<f32> {
    let b=c+delta; let n=i32(shore.options.z);
    if any(b<vec2(0)) || any(b>=vec2(n)) {
        if model_open_outside(b) { return max(0.0,foam_transport_velocity(c)[axis]*f32(delta[axis]))*foam_state(c); }
        return vec3(0.0);
    }
    if collider_blocks_face(c,b) { return vec3(0.0); }
    let normal=f32(delta[axis]);
    let a_speed=foam_transport_velocity(c)[axis]*normal;
    let b_speed=foam_transport_velocity(b)[axis]*normal;
    let signal=max(abs(a_speed),abs(b_speed));
    let outflow=select(0.0,0.5*(a_speed+signal),state(c).x>shore.density.z);
    let inflow=select(0.0,0.5*(b_speed-signal),state(b).x>shore.density.z);
    // Local Rusanov flux: identical positive donor weights for amount and
    // momentum, antisymmetric across a shared face. Unlike cell-speed splitting,
    // this resolves dilution at stagnation points inside an expanding flow.
    return outflow*foam_state(c)+inflow*foam_state(b);
}
// Positivity: the cell is the mean of its four nonnegative face states.
// dt * outward Rusanov donor weight / axis spacing must be <= 1/4 per face.
// Production's 0.2 CFL with the 12 m/s bound satisfies this (also for RK stages).
fn transport_foam(c: vec2<i32>) -> vec3<f32> {
    let spacing=shore.domain.zw/shore.options.z;
    var change=vec3(0.0);
    if second_order_foam() {
        let center=foam_cell(c);
        change=foam_axis_flux(center,0u)/spacing.x+foam_axis_flux(center,1u)/spacing.y;
    } else {
        change=(foam_flux(c,vec2(1,0),0u)+foam_flux(c,vec2(-1,0),0u))/spacing.x
            +(foam_flux(c,vec2(0,1),1u)+foam_flux(c,vec2(0,-1),1u))/spacing.y;
    }
    var foam=foam_state(c)-shore.timing.y*change;
    if foam.x<=0.0 { foam=vec3(0.0); }
    return foam;
}
fn source_duration(rate: f32, dt: f32) -> f32 {
    // Avoid cancellation for small rate*dt, including disabled decay/drag.
    let x=rate*dt;
    if x<0.001 { return dt*(1.0-0.5*x+x*x/6.0); }
    return (1.0-exp(-x))/rate;
}
// Exact local N'=source-a*N, R'=fraction*a*N-b*R. The fresh half-life
// includes conversion. This is applied ONCE after a completed SWE substep.
fn dual_foam_sources(amount: vec2<f32>, source: f32, dt: f32, rates: vec3<f32>) -> vec2<f32> {
    let a=rates.x; let b=rates.y; let fraction=rates.z;
    let ea=exp(-a*dt); let eb=exp(-b*dt);
    // Stable divided difference, including equal/near-equal exit rates.
    let transfer=exp(-min(a,b)*dt)*source_duration(abs(a-b),dt);
    var source_transfer=source_duration(b,dt)-transfer;
    if max(a,b)*dt<0.05 {
        let x=a*dt; let y=b*dt;
        source_transfer=dt*x*(0.5-(x+y)/6.0+(x*x+x*y+y*y)/24.0
            -(x*x*x+x*x*y+x*y*y+y*y*y)/120.0);
    }
    return max(vec2(0.0),vec2(amount.x*ea+source*source_duration(a,dt),
        amount.y*eb+fraction*(a*amount.x*transfer+source*source_transfer)));
}
fn dual_transport_velocity(c: vec2<i32>) -> vec2<f32> {
    var speed=velocity(state(c));
    if shore.solver.y>1.5 {
        let n=i32(shore.options.z);
        speed=0.5*(speed+velocity(initial_shore[u32(n*n+c.y*n+c.x)]));
    }
    return speed;
}
fn dual_closed_face(c: vec2<i32>, donor: vec2<i32>) -> bool {
    let n=i32(shore.options.z);
    if any(donor<vec2(0)) || any(donor>=vec2(n)) { return !model_open_outside(donor); }
    return collider_blocks_face(c,donor);
}
fn dual_backtrace(c: vec2<i32>, speed: vec2<f32>) -> vec2<f32> {
    var offset=speed*shore.timing.y/(shore.domain.zw/shore.options.z);
    // Production CFL keeps each displacement below one cell. Stop the normal
    // component at a closed face; do not interpolate through emergent solids.
    let step=vec2<i32>(sign(-offset));
    if dual_closed_face(c,c+vec2(step.x,0)) { offset.x=0.0; }
    if dual_closed_face(c,c+vec2(0,step.y)) { offset.y=0.0; }
    return vec2<f32>(c)-offset;
}
fn transport_dual_foam(c: vec2<i32>, divergence: f32) -> vec2<f32> {
    let grid=dual_backtrace(c,dual_transport_velocity(c));
    let base=vec2<i32>(floor(grid)); let f=fract(grid);
    let n=i32(shore.options.z); let cells=u32(n*n);
    var amounts=vec2(0.0);
    for(var y=0;y<2;y++) { for(var x=0;x<2;x++) {
        let donor=base+vec2(x,y);
        if any(donor<vec2(0)) || any(donor>=vec2(n)) { continue; }
        let i=u32(donor.y*n+donor.x);
        let water=initial_shore[cells+i];
        if water.x<=shore.density.z { continue; }
        // Both legs of a diagonal must be open; no corner-cutting through rock.
        if dual_closed_face(c,vec2(donor.x,c.y)) || dual_closed_face(c,vec2(c.x,donor.y))
            || dual_closed_face(vec2(donor.x,c.y),donor) || dual_closed_face(vec2(c.x,donor.y),donor) { continue; }
        let residual=clamp(initial_shore[4u*cells+i].w,0.0,water.w);
        let w=select(1.0-f.x,f.x,x==1)*select(1.0-f.y,f.y,y==1);
        amounts+=w*vec2(max(0.0,water.w-residual),residual);
    }}
    // SL interpolation alone transports a scalar, not an area density. Apply
    // compression/dilation once, to BOTH species; never on the conservative path.
    return amounts*exp(-divergence*shore.timing.y);
}
fn foam_sources(foam: vec3<f32>, water: vec2<f32>, source: f32) -> vec3<f32> {
    let dt=shore.timing.y; let decay=shore.timing.z;
    let remaining=exp(-decay*dt);
    let source_time=source_duration(decay,dt);
    let amount=foam.x*remaining+source*source_time;
    let birth_velocity=water*shore.foam_inertia.y;
    let desired=water*shore.foam_motion.x;
    if shore.foam_inertia.x>0.5 && shore.foam_motion.y<=0.0 {
        return vec3(amount,amount*desired);
    }
    var drag=0.0;
    if shore.foam_inertia.x>0.5 { drag=1.0/max(shore.foam_motion.y,0.000001); }
    let response=exp(-drag*dt);
    // Exact frozen-coefficient integration of generation, drag and decay.
    // Decay removes amount and its momentum together; it cannot accelerate foam.
    let momentum=remaining*(foam.yz*response+foam.x*desired*(1.0-response))
        +source*(desired*source_time+(birth_velocity-desired)*source_duration(decay+drag,dt));
    if amount<=0.0 { return vec3(0.0); }
    return vec3(amount,momentum);
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
// Load the water/bed stencil once per axis; reconstruction and its matching
// bed source share these values. Foam retains its independent transport flux.
struct WaterCell { position: vec2<i32>, value: vec4<f32>, depth: f32, bottom: f32 }
fn water_cell(c: vec2<i32>) -> WaterCell {
    let q=state(c); let z=bed(c);
    return WaterCell(c,vec4(q.x+z,velocity(q),0.0),q.x,z);
}
fn limited(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return select(vec4(0.0), sign(a)*min(min(1.3*abs(a),1.3*abs(b)),0.5*abs(a+b)), a*b>vec4(0.0));
}
struct Reconstruction { value: vec4<f32>, slope: vec4<f32>, bottom: f32, bed_slope: f32 }
fn reconstruct(left: WaterCell, center: WaterCell, right: WaterCell) -> Reconstruction {
    let n=i32(shore.options.z);
    let v=center.value; let z=center.bottom;
    var r=Reconstruction(v,vec4(0.0),z,0.0);
    if any(center.position<=vec2(0)) || any(center.position>=vec2(n-1)) { return r; }
    if min(center.depth,min(left.depth,right.depth))<0.02 { return r; }
    r.slope=limited(v-left.value,right.value-v);
    r.bed_slope=limited(vec4(z-left.bottom),vec4(right.bottom-z)).x;
    let dh=r.slope.x-r.bed_slope;
    if center.depth-0.5*abs(dh)<0.005 { r.slope=vec4(0.0); r.bed_slope=0.0; }
    return r;
}
// Outward hydrostatic flux with the one-sided pressure correction.
fn reconstructed_face(a: Reconstruction, b: Reconstruction, sign: f32, axis: u32) -> vec4<f32> {
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
fn bed_source(r: Reconstruction) -> f32 {
    let hl=max(r.value.x-0.5*r.slope.x-r.bottom+0.5*r.bed_slope,0.0);
    let hr=max(r.value.x+0.5*r.slope.x-r.bottom-0.5*r.bed_slope,0.0);
    return -4.905*(hl+hr)*r.bed_slope;
}
struct WaterAxis { flux: vec4<f32>, source: f32 }
fn water_axis(center: WaterCell, axis: u32) -> WaterAxis {
    var d=vec2(0); d[axis]=1;
    let a=water_cell(center.position-d); let b=water_cell(center.position+d);
    let aa=water_cell(center.position-2*d); let bb=water_cell(center.position+2*d);
    let middle=reconstruct(a,center,b);
    let left=reconstruct(aa,a,center); let right=reconstruct(center,b,bb);
    let flux=reconstructed_face(middle,right,1.0,axis)+reconstructed_face(middle,left,-1.0,axis);
    return WaterAxis(flux,bed_source(middle));
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
fn foam_flow_cell(c: vec2<i32>, minimum_depth: f32) -> vec3<f32> {
    let q=state(c);
    let wet=select(0.0,1.0,q.x>minimum_depth);
    return vec3(velocity(q)*wet,wet);
}
fn foam_flow_at(grid: vec2<f32>, minimum_depth: f32) -> vec3<f32> {
    let c=vec2<i32>(floor(grid)); let f=fract(grid);
    // Normalize wet contributors: dry zero velocities must not fake a collision.
    return mix(mix(foam_flow_cell(c,minimum_depth),foam_flow_cell(c+vec2(1,0),minimum_depth),f.x),
        mix(foam_flow_cell(c+vec2(0,1),minimum_depth),foam_flow_cell(c+vec2(1,1),minimum_depth),f.x),f.y);
}
fn foam_velocity_weight(strength: f32) -> f32 {
    let threshold=shore.foam_velocity.y;
    let width=shore.foam_velocity.z;
    if width<=0.0 { return select(0.0,1.0,strength>threshold); }
    return smoothstep(threshold,threshold+width,strength);
}
fn foam_front_weight(c: vec2<i32>) -> f32 {
    if shore.foam_front.x<=0.0 { return 0.0; }
    let center=state(c); let flow=velocity(center); let speed=length(flow);
    // A stationary surface has no forward direction; suppress tiny velocity noise.
    if speed<=0.02 || center.x<=0.00001 { return 0.0; }
    let spacing=shore.domain.zw/shore.options.z;
    let direction=flow/speed;
    let offset=direction*shore.foam_front.y/spacing;
    var velocity_strength=0.0;
    for(var sample=1u;sample<=4u;sample++) {
        let fraction=f32(sample)/4.0;
        let ahead=vec2<f32>(c)+offset*fraction;
        // Leaving the patch is not evidence of a collision.
        if any(ahead<vec2(0.0)) || any(ahead>vec2(shore.options.z-1.0)) { continue; }
        let sampled=foam_flow_at(ahead,0.00001);
        if sampled.z<=0.000001 { continue; }
        let difference=flow-sampled.xy/sampled.z;
        // Only closing motion contributes, including same-direction catch-up.
        let relative_speed=max(0.0,dot(difference,direction));
        velocity_strength=max(velocity_strength,relative_speed/(shore.foam_front.y*fraction));
    }
    // Deliberately no dry-front, rising-water or shore-direction requirement.
    return foam_velocity_weight(velocity_strength)*smoothstep(0.02,0.1,speed);
}
// Local directional compression ridge, not a 2D point maximum or an FFT-J mask.
// Five velocities from ONE immutable stage share the center direction. Sampling
// crosses one grid cell along the dominant axis; all donors must be wet so a dry
// wall/edge cannot masquerade as a velocity drop. No new water forcing is added.
fn foam_ridge_weight(c: vec2<i32>) -> f32 {
    let center=state(c); let flow=velocity(center); let speed=length(flow);
    if center.x<=shore.density.z || speed<=0.02 { return 0.0; }
    let direction=flow/speed;
    let spacing=shore.domain.zw/shore.options.z;
    let step=1.0/max(abs(direction.x)/spacing.x,abs(direction.y)/spacing.y);
    let offset=direction*step/spacing;
    var speeds: array<f32,5>;
    for(var i=0;i<5;i++) {
        let grid=vec2<f32>(c)+f32(i-2)*offset;
        if any(grid<vec2(0.0)) || any(grid>vec2(shore.options.z-1.0)) { return 0.0; }
        let sample=foam_flow_at(grid,shore.density.z);
        if sample.z<0.99999 { return 0.0; }
        speeds[i]=dot(sample.xy/sample.z,direction);
    }
    // Signed compression /s; expansion must not become a candidate peak.
    let left=(speeds[0]-speeds[2])/(2.0*step);
    let middle=(speeds[1]-speeds[3])/(2.0*step);
    let right=(speeds[2]-speeds[4])/(2.0*step);
    if middle<=shore.foam_velocity.y { return 0.0; }
    let bend=2.0*middle-left-right;
    if bend<=max(0.000001,shore.foam_ridge.z*middle) { return 0.0; }
    // Quadratic peak position along the normal, in meters. Reject extrapolated
    // maxima beyond the stencil. Flat compressive ramps intentionally emit none.
    let peak=0.5*step*(right-left)/bend;
    if abs(peak)>step { return 0.0; }
    let half_width=0.5*clamp(shore.foam_ridge.y,step,2.0*step);
    let band=1.0-smoothstep(max(0.0,half_width-0.5*step),half_width+0.5*step,abs(peak));
    return band*foam_velocity_weight(middle)*smoothstep(0.02,0.1,speed);
}
@group(0) @binding(6) var<uniform> previous_bed: ShoreParams;

// Static scene edits remove newly occluded water but never create water when
// lowering the bed. Vacated cells refill through normal SWE fluxes on resume.
@compute @workgroup_size(8,8)
fn edit_colliders(@builtin(global_invocation_id) id: vec3<u32>) {
    let n=u32(shore.options.z); let cells=n*n;
    if id.x>=n || id.y>=n { return; }
    let i=id.y*n+id.x;
    for(var block=0u;block<5u;block++) {
        new_shore[block*cells+i]=old_shore[block*cells+i];
    }
    let c=vec2<i32>(id.xy);
    let before=cell_bottom(c,previous_bed); let after=bed(c);
    if before==after { return; }
    var q=old_shore[cells+i];
    let old_depth=q.x;
    q.x=min(old_depth,max(0.0,before+old_depth-after));
    if q.x<0.00001 { q.x=0.0; }
    let depth_ratio=q.x/max(old_depth,0.0000001);
    q.y=q.y*depth_ratio;
    q.z=q.z*depth_ratio;
    if q.x<=shore.density.z {
        // Clear foam momentum/texture in newly dry cells; preserve wetness.
        q.w=0.0;
        new_shore[3u*cells+i]=vec4(0.0);
        new_shore[4u*cells+i]=vec4(0.0,0.0,old_shore[4u*cells+i].z,0.0);
    }
    let v=velocity(q);
    new_shore[i]=vec4(q.w,old_shore[i].y,q.x,v.x);
    new_shore[cells+i]=q;
    new_shore[2u*cells+i]=vec4(0.0,0.0,q.x+after,v.y);
}

@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = u32(shore.options.z); let cells=n*n;
    if id.x>=n || id.y>=n { return; }
    let index=id.y*n+id.x; let c=vec2<i32>(id.xy);
    let p=world(c); let z=bed(c); let dt=shore.timing.y;
    if !shore_active(shore) {
        new_shore[index]=vec4(0.0); new_shore[cells+index]=vec4(0.0); new_shore[2u*cells+index]=vec4(0.0);
        new_shore[3u*cells+index]=vec4(0.0); new_shore[4u*cells+index]=vec4(0.0); return;
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
        new_shore[2u*cells+index]=vec4(0.0,0.0,h+z,v.y);
        new_shore[3u*cells+index]=vec4(0.0); new_shore[4u*cells+index]=vec4(0.0); return;
    }
    if dt==0.0 {
        new_shore[index]=old_shore[index]; new_shore[cells+index]=old_shore[cells+index];
        new_shore[2u*cells+index]=old_shore[2u*cells+index];
        new_shore[3u*cells+index]=old_shore[3u*cells+index];
        new_shore[4u*cells+index]=old_shore[4u*cells+index]; return;
    }
    let previous=state(c);
    let original=initial_shore[cells+index];
    let spacing=shore.domain.zw/f32(n);
    var flux_x: vec4<f32>; var flux_z: vec4<f32>; var bed_sources=vec2(0.0);
    if second_order_swe() {
        let center=WaterCell(c,vec4(previous.x+z,velocity(previous),0.0),previous.x,z);
        let x=water_axis(center,0u); let z_axis=water_axis(center,1u);
        flux_x=x.flux; flux_z=z_axis.flux; bed_sources=vec2(x.source,z_axis.source);
    } else {
        flux_x=first_order_face(c,vec2(1,0),0u)+first_order_face(c,vec2(-1,0),0u);
        flux_z=first_order_face(c,vec2(0,1),1u)+first_order_face(c,vec2(0,-1),1u);
    }
    var q=previous-dt*(flux_x/spacing.x+flux_z/spacing.y);
    // Amount and momentum share one conservative flux, with no extra growth
    // term for convergence. The 12 m/s speed bound is included in the SWE CFL.
    var foam=vec3(previous.w,0.0,0.0);
    if shore.foam_lifetime.x<0.5 { foam=transport_foam(c); }
    q.w=foam.x;
    q.y+=dt*bed_sources.x/spacing.x; q.z+=dt*bed_sources.y/spacing.y;
    // One-way FFT boundary relaxation; outgoing SWE disturbances are damped here.
    let edge=select(smoothstep(0.80,1.0,(f32(id.x)+0.5)/f32(n)),model_edge_weight(p,shore),shore.model.x>0.5);
    if edge>0.0 {
        let incoming=incident(p,max(-z,0.1));
        let imposed=vec4(max(incoming.x-z,0.0),incoming.yz,0.0);
        if second_order_swe() {
            q+=dt*edge*4.0*(imposed-previous);
            foam.y-=dt*edge*4.0*foam_state(c).y;
            foam.z-=dt*edge*4.0*foam_state(c).z;
        } else {
            let remaining=exp(-dt*edge*4.0);
            q=mix(q,imposed,1.0-remaining);
            foam.y*=remaining; foam.z*=remaining;
        }
    }
    q.x=max(q.x,0.0);
    if q.x<0.00001 { q=vec4(0.0,0.0,0.0,q.w); }
    let v=velocity(q);
    let friction=9.81*0.03*0.03*length(v)/pow(max(q.x,0.005),1.3333333);
    if second_order_swe() {
        let f=9.81*0.03*0.03*length(velocity(previous))/pow(max(previous.x,0.005),1.3333333);
        q.y-=min(dt*f,1.0)*previous.y; q.z-=min(dt*f,1.0)*previous.z;
        if shore.solver.y>1.5 {
            q=0.5*(original+q);
            foam.y=0.5*(initial_motion_at(c).x+foam.y);
            foam.z=0.5*(initial_motion_at(c).y+foam.z);
        }
    } else { q.y=q.y/(1.0+dt*friction); q.z=q.z/(1.0+dt*friction); }
    q.x=max(q.x,0.0);
    if q.x<0.00001 { q=vec4(0.0,0.0,0.0,q.w); }
    let speed=length(velocity(q));
    q.y=q.y*min(1.0,12.0/max(speed,0.00001)); q.z=q.z*min(1.0,12.0/max(speed,0.00001));
    if shore.solver.y>0.5 && shore.solver.y<1.5 {
        new_shore[index]=initial_shore[index]; new_shore[cells+index]=q;
        new_shore[2u*cells+index]=initial_shore[2u*cells+index];
        new_shore[3u*cells+index]=vec4(foam.yz,initial_shore[3u*cells+index].zw);
        new_shore[4u*cells+index]=initial_shore[4u*cells+index]; return;
    }
    let wave_speed=sqrt(9.81*max(q.x,0.001));
    let rising=max(0.0,(q.x-original.x)/dt);
    let breaking=foam_breaking_weight(rising/wave_speed);
    let bore=smoothstep(1.0,1.4,length(velocity(q))/wave_speed);
    let divergence=(water_face_velocity(c,vec2(1,0),0u)+water_face_velocity(c,vec2(-1,0),0u))/spacing.x
        +(water_face_velocity(c,vec2(0,1),1u)+water_face_velocity(c,vec2(0,-1),1u))/spacing.y;
    // SWE cannot resolve overturning/air entrainment. Rising water plus a
    // compressive bore is a tunable proxy; shallow fast retreat alone emits none.
    let compression=smoothstep(shore.density.x,shore.density.x+0.5,max(0.0,-divergence));
    let background_source=shore.options.x*shore.foam_front.w*breaking*mix(0.25,1.0,max(bore,compression));
    var front_weight=0.0;
    var background=background_source;
    if shore.foam_ridge.x>0.5 {
        front_weight=foam_ridge_weight(c);
        background=0.0; // Ridge mode is front-only even with a legacy background setting.
    } else {
        front_weight=foam_front_weight(c);
    }
    let front_source=shore.foam_front.x*front_weight;
    // Inject once per completed SWE substep; transport handles convergence.
    // Stopping emission never erases existing amount or momentum.
    var source=(background+front_source)*smoothstep(0.003,0.03,q.x)*(1.0-edge);
    if shore.foam_ridge.w<0.5 { source=0.0; }
    var residual=0.0;
    if shore.foam_lifetime.x>0.5 {
        let transported=transport_dual_foam(c,divergence)*exp(-dt*edge*4.0);
        let amounts=dual_foam_sources(transported,source,dt,shore.foam_lifetime.yzw);
        residual=amounts.y;
        foam=vec3(amounts.x+amounts.y,(amounts.x+amounts.y)*velocity(q));
    } else {
        foam=foam_sources(vec3(q.w,foam.yz),velocity(q),source);
    }
    q.w=foam.x;
    let old=initial_shore[index];
    // Foam exists only on water; drying removes it without leaving sand residue.
    if q.x<=shore.density.z { q.w=0.0; foam.y=0.0; foam.z=0.0; residual=0.0; }
    var motion=advance_foam_motion(c,q,original,foam.yz);
    motion.texture.w=residual;
    new_shore[3u*cells+index]=motion.motion;
    new_shore[4u*cells+index]=motion.texture;
    let wetness=select(old.y*exp(-dt/shore.timing.w),1.0,q.x>0.003);
    new_shore[index]=vec4(q.w,wetness,q.x,velocity(q).x);
    new_shore[cells+index]=q;
    // Diagnostic channels for experimental/debug mode; z/w remain water data.
    let diagnostics=select(vec2(0.0),vec2(source,front_weight),(shore.foam_ridge.x>0.5 || shore.options.y>6.5) && q.x>shore.density.z);
    new_shore[2u*cells+index]=vec4(diagnostics,q.x+z,velocity(q).y);
}

