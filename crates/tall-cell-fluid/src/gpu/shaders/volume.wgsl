// Separate volume reduction kernel avoids mixing barriers with transport's per-cell early exits and branches.
struct Params {
    dims: vec4<u32>, offsets: vec4<u32>, metric: vec4<f32>,
    gravity: vec4<f32>, control: vec4<u32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage,read> state: array<f32>;
@group(0) @binding(2) var<storage,read_write> statistics: array<f32>;
var<workgroup> partial: array<f32,64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id:vec3<u32>,
    @builtin(local_invocation_id) local:vec3<u32>,
    @builtin(workgroup_id) group:vec3<u32>) {
    let n=id.x;
    var v=0.0;
    if params.control.x==7u {
        // All lanes run in the last group too; clamp read indices and mask invalid lanes.
        let cell=min(n,params.dims.w-1u);
        let fraction=clamp(0.5-(state[cell]+statistics[0])/params.metric.x,0.0,1.0);
        v=select(0.0,fraction,n<params.dims.w && state[params.dims.w+cell]>=0.0);
    } else {
        let count=(params.dims.w+63u)/64u;
        for(var i=local.x;i<count;i+=64u) {v+=statistics[4u+i];}
    }
    partial[local.x]=v;
    workgroupBarrier();
    for(var stride=32u;stride>0u;stride/=2u) {
        if local.x<stride {partial[local.x]+=partial[local.x+stride];}
        workgroupBarrier();
    }
    if local.x==0u {
        if params.control.x==7u {statistics[4u+group.x]=partial[0];}
        else if statistics[3]==0.0 {
            if abs(partial[0]-params.metric.w)<=max(1e-5,params.metric.w*1e-7) {
                statistics[3]=1.0;
            } else {
                if partial[0]>params.metric.w {statistics[1]=statistics[0];}
                else {statistics[2]=statistics[0];}
                statistics[0]=0.5*(statistics[1]+statistics[2]);
            }
        }
    }
}
