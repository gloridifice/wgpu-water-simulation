// Backtrace lengths are in grid spacings; phi and solid remain in meters and velocity in m/s.
struct Params {
    dims: vec4<u32>,
    offsets: vec4<u32>,
    metric: vec4<f32>, // h,dt,rho,target volume/h^3
    gravity: vec4<f32>,
    control: vec4<u32>, // mode
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> input_state: array<f32>;
@group(0) @binding(2) var<storage, read_write> output_state: array<f32>;
@group(0) @binding(3) var<storage, read> valid_in: array<u32>;
@group(0) @binding(4) var<storage, read_write> valid_out: array<u32>;
@group(0) @binding(5) var<storage, read> original: array<f32>;
@group(0) @binding(6) var<storage, read_write> statistics: array<f32>;

fn index(c: vec3<u32>, d: vec3<u32>) -> u32 { return c.x + d.x * (c.y + d.y*c.z); }
fn coord(n: u32, d: vec3<u32>) -> vec3<u32> { return vec3(n%d.x,(n/d.x)%d.y,n/(d.x*d.y)); }
fn face_dims(axis:u32) -> vec3<u32> { var d=params.dims.xyz; d[axis]+=1u; return d; }
fn axis_of(n:u32) -> u32 {
    if n<params.offsets.y {return 0u;}
    if n<params.offsets.z {return 1u;}
    return 2u;
}
fn liquid(c:vec3<u32>) -> bool {
    let n=index(c,params.dims.xyz);
    return input_state[n]<0.0 && input_state[params.dims.w+n]>=0.0;
}
fn open_face(axis:u32,c:vec3<u32>) -> bool {
    if c[axis]==0u || c[axis]==params.dims[axis] {return false;}
    var lo=c; lo[axis]-=1u;
    return input_state[params.dims.w+index(lo,params.dims.xyz)]>=0.0 &&
        input_state[params.dims.w+index(c,params.dims.xyz)]>=0.0;
}
fn fluid_face(axis:u32,c:vec3<u32>) -> bool {
    if !open_face(axis,c) {return false;}
    var lo=c; lo[axis]-=1u;
    return liquid(lo)||liquid(c);
}
fn sample_field(base:u32,d:vec3<u32>,p:vec3<f32>) -> f32 {
    let a=clamp(p,vec3(0.0),vec3<f32>(d-vec3(1u)));
    let lo=vec3<u32>(floor(a)); let hi=min(lo+vec3(1u),d-vec3(1u));
    let t=a-vec3<f32>(lo);
    var result=0.0;
    for(var k=0u;k<2u;k++) { for(var j=0u;j<2u;j++) { for(var i=0u;i<2u;i++) {
        let c=vec3(select(lo.x,hi.x,i==1u),select(lo.y,hi.y,j==1u),select(lo.z,hi.z,k==1u));
        let w=select(1.0-t.x,t.x,i==1u)*select(1.0-t.y,t.y,j==1u)*select(1.0-t.z,t.z,k==1u);
        result+=w*input_state[base+index(c,d)];
    }}}
    return result;
}
fn velocity(p:vec3<f32>) -> vec3<f32> {
    var u=vec3(0.0);
    for(var axis=0u;axis<3u;axis++) {
        var c=p-vec3(0.5); c[axis]+=0.5;
        u[axis]=sample_field(params.offsets[axis],face_dims(axis),c);
    }
    return u;
}
fn sample_solid(p:vec3<f32>) -> f32 { return sample_field(params.dims.w,params.dims.xyz,p-vec3(0.5)); }
fn backtrace(p:vec3<f32>) -> vec3<f32> {
    let scale=params.metric.y/params.metric.x;
    let mid=clamp(p-0.5*scale*velocity(p),vec3(0.0),vec3<f32>(params.dims.xyz));
    let end=clamp(p-scale*velocity(mid),vec3(0.0),vec3<f32>(params.dims.xyz));
    if sample_solid(p)<0.0 {return p;}
    var previous=p;
    for(var part=1u;part<=4u;part++) {
        let point=mix(p,end,f32(part)*0.25);
        if sample_solid(point)<0.0 {
            var safe=previous; var blocked=point;
            for(var iteration=0u;iteration<20u;iteration++) {
                let q=0.5*(safe+blocked);
                if sample_solid(q)<0.0 {blocked=q;} else {safe=q;}
            }
            return safe;
        }
        previous=point;
    }
    return end;
}
fn far_distance() -> f32 { return length(vec3<f32>(params.dims.xyz))*params.metric.x; }
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    let n=id.x; let mode=params.control.x;
    if n>=params.offsets.w {return;}
    output_state[n]=input_state[n]; valid_out[n]=valid_in[n];
    if n<params.dims.w {
        let c=coord(n,params.dims.xyz);
        if mode==2u {output_state[n]=sample_field(0u,params.dims.xyz,backtrace(vec3<f32>(c)+vec3(0.5))-vec3(0.5));}
        if mode==4u {
            var fixed=original[n]==0.0; var distance=far_distance();
            var gradient=vec3(0.0);
            for(var axis=0u;axis<3u;axis++) {
                var lo=c; var hi=c;
                lo[axis]=u32(max(i32(c[axis])-1,0)); hi[axis]=min(c[axis]+1u,params.dims[axis]-1u);
                let low=original[index(lo,params.dims.xyz)]; let high=original[index(hi,params.dims.xyz)];
                gradient[axis]=(high-low)/(f32(hi[axis]-lo[axis])*params.metric.x);
                if (low<0.0)!=(original[n]<0.0) || (high<0.0)!=(original[n]<0.0) || low==0.0 || high==0.0 {fixed=true;}
                for(var direction=-1;direction<=1;direction+=2) {
                    let j=i32(c[axis])+direction;
                    if j<0 || j>=i32(params.dims[axis]) {continue;}
                    var q=c; q[axis]=u32(j); let phi=original[index(q,params.dims.xyz)];
                    if (phi<0.0)!=(original[n]<0.0) || phi==0.0 {
                        distance=min(distance,params.metric.x*abs(original[n])/max(abs(original[n])+abs(phi),1e-30));
                    }
                }
            }
            if fixed && length(gradient)>1e-12 {distance=abs(original[n])/length(gradient);}
            if original[n]==0.0 {distance=0.0;}
            output_state[n]=distance; valid_out[n]=select(0u,1u,fixed);
        }
        if mode==5u && valid_in[n]==0u {
            var adjacent=vec3(far_distance());
            for(var axis=0u;axis<3u;axis++) {for(var direction=-1;direction<=1;direction+=2) {
                let j=i32(c[axis])+direction;
                if j<0 || j>=i32(params.dims[axis]) {continue;}
                var q=c; q[axis]=u32(j); adjacent[axis]=min(adjacent[axis],input_state[index(q,params.dims.xyz)]);
            }}
            let a=min(adjacent.x,min(adjacent.y,adjacent.z));
            let b=max(min(adjacent.x,adjacent.y),min(max(adjacent.x,adjacent.y),adjacent.z));
            let cmax=max(adjacent.x,max(adjacent.y,adjacent.z));
            var d=a+params.metric.x;
            if d>b {d=(a+b+sqrt(max(0.0,2.0*params.metric.x*params.metric.x-(a-b)*(a-b))))/2.0;}
            if d>cmax {
                let sum=a+b+cmax; let squares=a*a+b*b+cmax*cmax;
                d=(sum+sqrt(max(0.0,sum*sum-3.0*(squares-params.metric.x*params.metric.x))))/3.0;
            }
            output_state[n]=min(input_state[n],d);
        }
        if mode==6u {
            if input_state[n]<far_distance() {output_state[n]=select(input_state[n],-input_state[n],original[n]<0.0);}
            else {output_state[n]=original[n];}
        }
        if mode==8u {output_state[n]=input_state[n]+statistics[0];}
        return;
    }
    if n<params.offsets.x {return;} // static solid field
    let axis=axis_of(n); let d=face_dims(axis); let c=coord(n-params.offsets[axis],d);
    if mode==0u {
        let valid=fluid_face(axis,c); valid_out[n]=select(0u,1u,valid);
        output_state[n]=select(0.0,input_state[n],valid); return;
    }
    if mode==1u && valid_in[n]==0u && open_face(axis,c) {
        var sum=0.0; var count=0u;
        for(var a=0u;a<3u;a++) {for(var direction=-1;direction<=1;direction+=2) {
            let j=i32(c[a])+direction;
            if j<0 || j>=i32(d[a]) {continue;}
            var q=c; q[a]=u32(j); let other=params.offsets[axis]+index(q,d);
            if valid_in[other]!=0u {sum+=input_state[other];count++;}
        }}
        if count>0u {output_state[n]=sum/f32(count);valid_out[n]=1u;}
    }
    if mode==2u {
        var p=vec3<f32>(c)+vec3(0.5);p[axis]-=0.5;
        output_state[n]=velocity(backtrace(p))[axis];
    }
    if mode==3u && fluid_face(axis,c) {output_state[n]+=params.metric.y*params.gravity[axis];}
    if !open_face(axis,c) {output_state[n]=0.0;valid_out[n]=0u;}
}
