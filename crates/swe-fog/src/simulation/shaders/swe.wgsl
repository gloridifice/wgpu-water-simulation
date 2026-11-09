// Hydrostatic reconstruction + shared Rusanov flux + limited MUSCL + SSP-RK2.
// State blocks: current/candidate, source, predictor; all values remain f32.
struct Params { domain:vec4f, physics:vec4f, wind:vec4f, initial:vec4f, noise:vec4f, options:vec4f, display:vec4f, request:vec4f }
struct Dispatch { mode:u32, stage:u32, axis:u32, unused:u32 }
struct Reconstruction { sx:vec4f, sz:vec4f, bed:vec4f, limiter:vec4f, correction:vec4f }
struct Face { flux:vec4f, correction:vec2f, pad:vec2f }
struct Control {
    remaining:f32, dt:f32, simulated_time:f32, max_rate_bits:atomic<u32>,
    role:u32, rejected:atomic<u32>, accepted:u32, retries:u32,
    dt_cap:f32, dropped_time:f32, fault:u32, closed:u32,
    min_height_bits:atomic<u32>, max_height_bits:atomic<u32>, max_speed_bits:atomic<u32>, pad:u32,
}
struct GasEvent { position_rotation:vec4f, footprint:vec4f, metadata:vec4u }
struct Events { header:vec4u, records:array<GasEvent> }
@group(0) @binding(0) var<uniform> p:Params;
@group(0) @binding(1) var<storage,read_write> states:array<vec4f>;
@group(0) @binding(2) var<storage,read> terrain:array<vec4f>;
@group(0) @binding(3) var<storage,read> passable:array<u32>;
@group(0) @binding(4) var<storage,read_write> recon:array<Reconstruction>;
@group(0) @binding(5) var<storage,read_write> faces:array<Face>;
@group(0) @binding(6) var<storage,read_write> ctl:Control;
@group(0) @binding(7) var<storage,read> events:Events;
@group(0) @binding(8) var published:texture_storage_2d<rgba32float,write>;
@group(0) @binding(9) var disturbance:texture_2d_array<f32>;
@group(0) @binding(10) var disturbance_sampler:sampler;
@group(1) @binding(0) var<uniform> dispatch:Dispatch;
var<private> size:vec2i;
var<private> cells:i32;
var<private> spacing:vec2f;
var<private> gravity:f32;
const EPS:f32=0.0001;
fn index_of(c:vec2i)->i32{return c.y*size.x+c.x;}
fn inside(c:vec2i)->bool{return all(c>=vec2i(0))&&all(c<size);}
fn current(i:i32)->vec4f{return states[i32(ctl.role)*cells+i];}
fn input_state(i:i32)->vec4f{return states[select(3,2,dispatch.stage==0u)*cells+i];}
fn velocity(q:vec4f)->vec2f{return q.yz/max(q.x,EPS);}
fn concentration(q:vec4f)->f32{return q.w/max(q.x,EPS);}
fn primitive(q:vec4f,b:f32)->vec4f{return vec4f(q.x+b,velocity(q),concentration(q));}
fn limited(a:vec4f,b:vec4f)->vec4f{return select(vec4f(0),sign(a)*min(min(1.3*abs(a),1.3*abs(b)),0.5*abs(a+b)),a*b>vec4f(0));}
fn hash_u(input:u32)->u32{var x=input;x=x^(x>>16u);x=x*0x7feb352du;x=x^(x>>15u);x=x*0x846ca68bu;return x^(x>>16u);}
fn lattice(v:vec2i)->f32{return f32(hash_u(bitcast<u32>(v.x)*92837111u^bitcast<u32>(v.y)*689287499u^u32(p.noise.x))&0x00ffffffu)/8388607.5-1.0;}
fn value_noise(v:vec2f)->f32{let i=vec2i(floor(v));var f=fract(v);f=f*f*(3.0-2.0*f);return mix(mix(lattice(i),lattice(i+vec2i(1,0)),f.x),mix(lattice(i+vec2i(0,1)),lattice(i+vec2i(1)),f.x),f.y);}
fn fbm(world:vec2f)->f32{var v=(world+p.options.xy)*p.initial.z;var sum=0.0;var weight=0.0;var amplitude=1.0;for(var o=0;o<i32(p.noise.y);o++){sum+=value_noise(v)*amplitude;weight+=amplitude;v=v*p.noise.z+vec2f(19.17,7.13);amplitude*=p.noise.w;}return sum/max(weight,0.000001);}
fn interaction_vector(e:GasEvent,i:i32)->vec2f{
    let c=vec2i(i%size.x,i/size.x);let world=p.domain.xy+(vec2f(c)+0.5)*spacing;let a=e.position_rotation.w;
    let rot=mat2x2f(cos(a),-sin(a),sin(a),cos(a));let uv=transpose(rot)*(world-e.position_rotation.xz)/e.footprint.xy+0.5;
    if any(uv<=vec2f(0))||any(uv>=vec2f(1)){return vec2f(0);}
    return rot*textureSampleLevel(disturbance,disturbance_sampler,uv,i32(e.metadata.x),0.0).rg*bitcast<f32>(e.metadata.z);
}
fn overlap(e:GasEvent,q:vec4f,i:i32)->f32{
    if terrain[i].y<0.5||q.x<=EPS{return 0.0;}
    let low=e.position_rotation.y-0.5*e.footprint.w;let high=e.position_rotation.y+0.5*e.footprint.w;let bed=terrain[i].x;
    return clamp((min(bed+q.x,high)-max(bed,low))/q.x,0.0,1.0);
}
fn impulse_step(input:vec4f,i:i32)->vec4f{
    var q=input;if terrain[i].y<0.5||q.x<=EPS{return q;}var dv=vec2f(0);
    for(var e=0u;e<events.header.x;e++){let event=events.records[e];if event.metadata.y==0u{dv+=interaction_vector(event,i)*event.footprint.z*overlap(event,q,i);}}
    return vec4f(q.x,q.yz+q.x*dv,q.w);
}
fn source_step(input:vec4f,duration:f32,i:i32)->vec4f{
    var q=input;var drag=1.0/p.physics.y;var weighted=p.wind.xy*drag;
    for(var e=0u;e<events.header.x;e++){let event=events.records[e];if event.metadata.y==1u{
        let desired=interaction_vector(event,i);let reference_speed=3.0*bitcast<f32>(event.metadata.z);
        let weight=event.footprint.z*overlap(event,q,i)*min(length(desired)/max(reference_speed,EPS),1.0);drag+=weight;weighted+=weight*desired;
    }}
    let desired=weighted/drag;q=vec4f(q.x,q.x*(desired+(velocity(q)-desired)*exp(-duration*drag)),q.w);
    if p.display.w>0.5{q.w*=exp(-0.6931471805599453*duration/p.physics.z);}return q;
}
fn face_index(f:vec2i,a:i32)->i32{if a==0{return f.y*(size.x+1)+f.x;}return (size.x+1)*size.y+f.y*size.x+f.x;}
fn ghost(input:vec4f,a:i32,low:bool)->vec4f{var q=input;if ctl.closed!=0u{q[a+1]=-q[a+1];return q;}let outflow=select(velocity(q)[a]>0.0,velocity(q)[a]<0.0,low);if outflow{return q;}return vec4f(p.wind.z,p.wind.z*p.wind.xy,p.wind.z*p.wind.w);}
fn flux(q:vec4f,a:i32)->vec4f{var f=q*velocity(q)[a];f[a+1]+=0.5*gravity*q.x*q.x;return f;}
fn rate(q:vec4f,i:i32)->f32{
    var v=max(abs(velocity(q)),abs(p.wind.xy));for(var e=0u;e<events.header.x;e++){let event=events.records[e];if event.metadata.y==1u&&event.footprint.z>0.0&&overlap(event,q,i)>0.0{v=max(v,abs(interaction_vector(event,i)));}}
    let wave=sqrt(gravity*max(q.x,0.0));return (v.x+wave)/spacing.x+(v.y+wave)/spacing.y;
}
fn bad(q:vec4f)->bool{return !all(abs(q)<vec4f(3.4e38))||q.x<0.0||q.w<0.0||length(velocity(q))>20.0;}
fn slope_at(i:i32,a:i32)->vec4f{if a==0{return recon[i].sx;}return recon[i].sz;}
fn compute_slopes(c:vec2i,i:i32){
    for(var a=0;a<2;a++){
        var slope=vec4f(0);var bed_slope=0.0;var d=vec2i(0);d[a]=1;let lo=c-d;let hi=c+d;
        if p.options.w>1.5&&inside(lo)&&inside(hi)&&terrain[i].y>0.5{
            let l=index_of(lo);let r=index_of(hi);let clear=passable[face_index(c,a)]!=0u&&passable[face_index(c+d,a)]!=0u;
            let q=input_state(i);let ql=input_state(l);let qr=input_state(r);
            if clear&&terrain[l].y>0.5&&terrain[r].y>0.5&&min(q.x,min(ql.x,qr.x))>0.002{
                let v=primitive(q,terrain[i].x);slope=limited(v-primitive(ql,terrain[l].x),primitive(qr,terrain[r].x)-v);
                bed_slope=limited(vec4f(terrain[i].x-terrain[l].x),vec4f(terrain[r].x-terrain[i].x)).x;
                if q.x-0.5*abs(slope.x-bed_slope)<EPS||v.w-0.5*abs(slope.w)<0.0{slope=vec4f(0);bed_slope=0.0;}
            }
        }
        if a==0{recon[i].sx=slope;}else{recon[i].sz=slope;}recon[i].bed[a]=bed_slope;
    }
}
fn compute_face(f:vec2i,a:i32){
    var d=vec2i(0);d[a]=1;let lc=f-d;let rc=f;let il=inside(lc);let ir=inside(rc);
    let li=index_of(clamp(lc,vec2i(0),size-1));let ri=index_of(clamp(rc,vec2i(0),size-1));
    var ql=input_state(li);var qr=input_state(ri);var bl=terrain[li].x;var br=terrain[ri].x;var vl=primitive(ql,bl);var vr=primitive(qr,br);
    if il{vl+=0.5*slope_at(li,a);bl+=0.5*recon[li].bed[a];}if ir{vr-=0.5*slope_at(ri,a);br-=0.5*recon[ri].bed[a];}
    if !il{ql=ghost(qr,a,true);bl=br;vl=primitive(ql,bl);}if !ir{qr=ghost(ql,a,false);br=bl;vr=primitive(qr,br);}
    let raw_l=max(vl.x-bl,0.0);let raw_r=max(vr.x-br,0.0);let fi=face_index(f,a);var result:Face;
    let wall=passable[fi]==0u||((!il||!ir)&&ctl.closed!=0u);
    if wall{result.correction=0.5*gravity*vec2f(raw_l*raw_l,raw_r*raw_r);}else{
        let top=max(bl,br);let hl=max(vl.x-top,0.0);let hr=max(vr.x-top,0.0);
        let left=vec4f(hl,vl.yz*hl,0);let right=vec4f(hr,vr.yz*hr,0);
        let speed=max(abs(velocity(left)[a])+sqrt(gravity*hl),abs(velocity(right)[a])+sqrt(gravity*hr));
        result.flux=0.5*(flux(left,a)+flux(right,a))-0.5*speed*(right-left);
        result.flux.w=result.flux.x*select(max(vr.w,0.0),max(vl.w,0.0),result.flux.x>=0.0);
        if p.physics.w>0.0&&il&&ir{result.flux.w-=p.physics.w*min(hl,hr)*(max(vr.w,0.0)-max(vl.w,0.0))/spacing[a];}
        result.correction=0.5*gravity*vec2f(raw_l*raw_l-hl*hl,raw_r*raw_r-hr*hr);
    }faces[fi]=result;
}
fn compute_limiter(c:vec2i,i:i32){
    var outgoing=vec2f(0);for(var a=0;a<2;a++){var d=vec2i(0);d[a]=1;let lo=faces[face_index(c,a)].flux;let hi=faces[face_index(c+d,a)].flux;outgoing+=(max(hi.xw,vec2f(0))+max(-lo.xw,vec2f(0)))/spacing[a];}
    let q=input_state(i);recon[i].limiter=vec4f(min(vec2f(1),0.999999*q.xw/max(ctl.dt*outgoing,vec2f(1e-30))),0,0);
}
fn limited_face(f:vec2i,a:i32)->vec4f{
    let value=faces[face_index(f,a)].flux;var d=vec2i(0);d[a]=1;let hd=select(f,f-d,value.x>=0.0);let md=select(f,f-d,value.w>=0.0);var factor=1.0;
    if value.x!=0.0&&inside(hd){factor=min(factor,recon[index_of(hd)].limiter.x);}if value.w!=0.0&&inside(md){factor=min(factor,recon[index_of(md)].limiter.y);}return factor*value;
}
fn rhs_step(c:vec2i,i:i32)->vec4f{
    var q=input_state(i);let b=terrain[i].x;
    for(var a=0;a<2;a++){var d=vec2i(0);d[a]=1;let lo=face_index(c,a);let hi=face_index(c+d,a);var fl=limited_face(c,a);var fr=limited_face(c+d,a);fl[a+1]+=faces[lo].correction.y;fr[a+1]+=faces[hi].correction.x;q-=ctl.dt*(fr-fl)/spacing[a];
        let eta=input_state(i).x+b;let ds=slope_at(i,a).x;let db=recon[i].bed[a];let hl=max(eta-0.5*ds-b+0.5*db,0.0);let hr=max(eta+0.5*ds-b-0.5*db,0.0);q[a+1]-=ctl.dt*0.5*gravity*(hl+hr)*db/spacing[a];
    }return q;
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id:vec3u){
    size=vec2i(textureDimensions(published));cells=size.x*size.y;spacing=p.domain.zw/vec2f(size);gravity=select(0.0,p.physics.x,p.display.z>0.5);
    let c=vec2i(id.xy);let i=index_of(c);let cell=inside(c);let one=all(c==vec2i(0));let mode=dispatch.mode;
    if mode==0u&&cell{
        let world=p.domain.xy+(vec2f(c)+0.5)*spacing;var h=p.initial.x+p.initial.y*fbm(world)+p.initial.w;if p.options.z>0.5{h-=terrain[i].x;}h=select(0.0,max(h,0.0),terrain[i].y>0.5);let q=vec4f(h,0,0,h);
        states[i]=q;states[cells+i]=q;states[2*cells+i]=q;states[3*cells+i]=q;recon[i].correction=vec4f(0);
    }
    if mode==1u&&one{ctl.remaining=0;ctl.dt=0;ctl.simulated_time=0;atomicStore(&ctl.max_rate_bits,0u);ctl.role=0u;atomicStore(&ctl.rejected,0u);ctl.accepted=0u;ctl.retries=0u;ctl.dt_cap=1e20;ctl.dropped_time=0;ctl.fault=0u;ctl.closed=u32(p.request.y);}
    if mode==2u&&one{ctl.closed=u32(p.request.y);let requested=ctl.remaining+p.request.x;ctl.dropped_time+=max(0.0,requested-0.1);ctl.remaining=min(0.1,requested);}
    if mode==16u&&cell&&ctl.fault==0u{states[i32(ctl.role)*cells+i]=impulse_step(current(i),i);}
    if mode==3u&&one{atomicStore(&ctl.max_rate_bits,0u);atomicStore(&ctl.rejected,0u);}
    if mode==4u&&cell&&terrain[i].y>0.5{atomicMax(&ctl.max_rate_bits,bitcast<u32>(rate(current(i),i)));}
    if mode==5u&&one{let r=bitcast<f32>(atomicLoad(&ctl.max_rate_bits));var diffusion_limit=1e20;if p.physics.w>0.0{diffusion_limit=0.2/(p.physics.w*(1.0/(spacing.x*spacing.x)+1.0/(spacing.y*spacing.y)));}ctl.dt=select(0.0,min(min(ctl.remaining,ctl.dt_cap),min(0.18/max(r,1e-10),diffusion_limit)),ctl.fault==0u);}
    if mode==6u&&cell&&ctl.dt>0.0{states[2*cells+i]=source_step(current(i),0.5*ctl.dt,i);}
    if mode==7u&&cell&&ctl.dt>0.0&&atomicLoad(&ctl.rejected)==0u{compute_slopes(c,i);}
    if mode==8u&&ctl.dt>0.0&&atomicLoad(&ctl.rejected)==0u{let a=i32(dispatch.axis);if (a==0&&c.x<=size.x&&c.y<size.y)||(a==1&&c.x<size.x&&c.y<=size.y){compute_face(c,a);}}
    if mode==9u&&cell&&ctl.dt>0.0&&atomicLoad(&ctl.rejected)==0u{compute_limiter(c,i);}
    if mode==10u&&cell&&ctl.dt>0.0&&atomicLoad(&ctl.rejected)==0u{
        var q=vec4f(0);if terrain[i].y>0.5{q=rhs_step(c,i);}
        if dispatch.stage==0u{states[3*cells+i]=q;}else{
            q=source_step(0.5*(states[2*cells+i]+q),0.5*ctl.dt,i);let fix=-min(q.xw,vec2f(0));recon[i].correction=vec4f(recon[i].correction.xy,fix);
            if q.x>= -1e-7{q.x=max(q.x,0.0);}if q.w>= -1e-7{q.w=max(q.w,0.0);}if q.x==0.0{q=vec4f(0,0,0,q.w);}states[i32(1u-ctl.role)*cells+i]=q;
        }
    }
    if mode==11u&&cell&&ctl.dt>0.0{let q=states[select(i32(1u-ctl.role),3,dispatch.stage==0u)*cells+i];if bad(q)||rate(q,i)*ctl.dt>0.200001{atomicOr(&ctl.rejected,1u);}}
    if mode==12u&&one&&ctl.dt>0.0{if atomicLoad(&ctl.rejected)==0u{ctl.role=1u-ctl.role;ctl.remaining=max(ctl.remaining-ctl.dt,0.0);ctl.simulated_time+=ctl.dt;ctl.dt_cap=1e20;ctl.accepted++;ctl.retries=0u;}else{ctl.dt_cap=0.5*ctl.dt;ctl.retries++;if ctl.retries>=64u||ctl.dt_cap<1e-8{ctl.fault=1u;}}}
    if mode==14u&&cell&&ctl.dt>0.0&&atomicLoad(&ctl.rejected)==0u{let correction=recon[i].correction;recon[i].correction=vec4f(correction.xy+correction.zw,correction.zw);}
    if mode==15u&&one{atomicStore(&ctl.min_height_bits,0x7f800000u);atomicStore(&ctl.max_height_bits,0u);atomicStore(&ctl.max_speed_bits,0u);}
    if mode==13u&&cell{let q=current(i);textureStore(published,c,vec4f(q.x,q.w,velocity(q)));if terrain[i].y>0.5{atomicMin(&ctl.min_height_bits,bitcast<u32>(max(q.x,0.0)));atomicMax(&ctl.max_height_bits,bitcast<u32>(max(q.x,0.0)));atomicMax(&ctl.max_speed_bits,bitcast<u32>(length(velocity(q))));}}
}
