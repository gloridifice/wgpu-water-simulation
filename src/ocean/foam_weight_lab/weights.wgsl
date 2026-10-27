// Diagnostic source weights only; no foam transport or density state is altered.
struct Params {
    sizes: vec4<u32>,
    domain: vec4<f32>,
    weights: vec4<f32>,
    display: vec4<f32>,
}
@group(0) @binding(0) var<uniform> p: Params;
// Reuses production Foam.field: x=unrelated accumulated coverage, y=current J.
@group(0) @binding(1) var<storage,read> jacobian: array<vec2<f32>>;
@group(0) @binding(2) var<storage,read_write> values: array<vec4<f32>>;
@group(0) @binding(3) var images: texture_storage_2d_array<rgba8unorm,write>;

fn mask(j: f32) -> f32 {
    let compression=max(0.0,1.0-j);
    if p.weights.y<=0.0 { return select(0.0,1.0,compression>p.weights.x); }
    return smoothstep(p.weights.x,p.weights.x+p.weights.y,compression);
}
fn sample_cell(cell: vec2<i32>) -> vec2<f32> {
    let n=i32(p.sizes.x); let q=((cell%vec2(n))+vec2(n))%vec2(n);
    let j=jacobian[u32(q.y*n+q.x)].y;
    return vec2(j,mask(j));
}
// Exact box integral of piecewise-constant FFT samples over the target cell.
// Each FFT sample owns a square centered on its undeformed grid position.
// Apply the nonlinear threshold BEFORE averaging: mask(mean J) would erase
// subcell compression. Upsampling cannot reconstruct missing FFT detail.
fn averaged(cell: vec2<u32>) -> vec2<f32> {
    let spacing=p.domain.yz/f32(p.sizes.y);
    let lower=(vec2<f32>(cell)*spacing-0.5*p.domain.yz)/p.domain.x*f32(p.sizes.x)+0.5*f32(p.sizes.x)+vec2(0.5);
    let upper=lower+spacing/p.domain.x*f32(p.sizes.x);
    var sum=vec2(0.0);
    for(var z=i32(floor(lower.y));z<i32(ceil(upper.y));z++) {
        let wz=max(0.0,min(upper.y,f32(z+1))-max(lower.y,f32(z)));
        for(var x=i32(floor(lower.x));x<i32(ceil(upper.x));x++) {
            let wx=max(0.0,min(upper.x,f32(x+1))-max(lower.x,f32(x)));
            sum+=wx*wz*sample_cell(vec2(x,z));
        }
    }
    return sum/((upper.x-lower.x)*(upper.y-lower.y));
}
fn j_color(j:f32) -> vec3<f32> {
    let neutral=vec3(0.88,0.90,0.94);
    if j<1.0 {return mix(neutral,vec3(1.0,0.25,0.06),clamp((1.0-j)/p.display.x,0.0,1.0));}
    return mix(neutral,vec3(0.04,0.35,0.95),clamp((j-1.0)/p.display.x,0.0,1.0));
}
fn weight_color(w:f32) -> vec3<f32> {
    if w<1.0 {return mix(vec3(0.015,0.025,0.07),vec3(0.04,0.75,0.65),w);}
    return mix(vec3(0.04,0.75,0.65),vec3(1.0,0.70,0.08),clamp((w-1.0)/max(p.display.y-1.0,0.001),0.0,1.0));
}
@compute @workgroup_size(8,8)
fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    let n=p.sizes.y;
    if id.x>=n || id.y>=n {return;}
    let feature=averaged(id.xy);
    let weight=mix(1.0,p.weights.w*feature.y,p.weights.z);
    let rate=p.domain.w*weight;
    values[id.y*n+id.x]=vec4(feature,weight,rate);
    // +X right, +Z up in all diagnostic views; values remain in solver row order.
    let xy=vec2<i32>(i32(id.x),i32(n-1u-id.y));
    textureStore(images,xy,0,vec4(j_color(feature.x),1.0));
    textureStore(images,xy,1,vec4(vec3(feature.y),1.0));
    textureStore(images,xy,2,vec4(weight_color(weight),1.0));
    textureStore(images,xy,3,vec4(vec3(clamp(rate/p.display.z,0.0,1.0)),1.0));
}
