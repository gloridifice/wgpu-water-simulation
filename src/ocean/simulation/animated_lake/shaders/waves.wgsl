struct LakeUniform { grid:vec4<f32>, options:vec4<f32>, fft:vec4<f32>, foam:vec4<f32>, foam_flow:vec4<f32> }
struct Wave { direction_birth_life:vec4<f32>, propagation:vec4<f32>, envelope:vec4<f32>, appearance:vec4<f32>, limits:vec4<f32> }
@group(0) @binding(0) var<uniform> lake:LakeUniform;
@group(0) @binding(1) var terrain:texture_2d<f32>;
@group(0) @binding(2) var<storage,read> waves:array<Wave>;
@group(0) @binding(3) var heights:texture_storage_2d<r32float,write>;
@group(0) @binding(4) var height_input:texture_2d<f32>;
@group(0) @binding(5) var field:texture_storage_2d<rgba32float,write>;
@group(0) @binding(6) var<storage,read> fields:array<vec2<f32>>;
fn fft_resolution() -> f32 { return lake.fft.x; }
fn bounded_fft_height(world:vec2<f32>, bed:f32, shore_distance:f32) -> f32 {
    if lake.fft.z<=0.0 || bed>=0.0 { return 0.0; }
    let grid=(world/lake.fft.y+vec2(0.5))*lake.fft.x;
    // Fade by physical depth/distance, not the selected propagation coordinate.
    // No horizontal displacement: the fixed wet mask and rock footprint stay aligned.
    var fade=1.0;
    if lake.fft.w>0.5 {
        fade=smoothstep(0.0,0.5,-bed)*smoothstep(0.0,1.0,shore_distance);
    }
    return sample_field(grid,0u)*lake.fft.z*fade;
}
fn coordinate(p:vec2<i32>) -> f32 {
    let c=textureLoad(terrain,clamp(p,vec2(0),vec2(i32(lake.grid.x)-1)),0);
    return select(c.y,c.z,lake.options.y>0.5);
}
@compute @workgroup_size(8,8)
fn height_main(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=vec2<u32>(u32(lake.grid.x))) { return; }
    let p=vec2<i32>(id.xy);
    let data=textureLoad(terrain,p,0);
    let world=(vec2<f32>(id.xy)/(lake.grid.x-1.0)-vec2(0.5))*lake.grid.y;
    let c=coordinate(p);
    let grad=length(vec2(coordinate(p+vec2(1,0))-coordinate(p-vec2(1,0)),coordinate(p+vec2(0,1))-coordinate(p-vec2(0,1))));
    var height=0.0;
    if data.x<0.0 {
        for(var i=0u;i<u32(lake.grid.w);i++) {
            let w=waves[i];let age=lake.grid.z-w.direction_birth_life.z;
            if age<0.0 || age>=w.direction_birth_life.w { continue; }
            let offset=world-w.envelope.xy;
            let angle=abs(atan2(offset.y*w.direction_birth_life.x-offset.x*w.direction_birth_life.y,dot(offset,w.direction_birth_life.xy)));
            let envelope=1.0-smoothstep(0.0,w.envelope.z,angle);
            let width=w.propagation.z*mix(w.envelope.w,1.0,envelope);
            let front=clamp(w.propagation.x+w.propagation.y*age,w.limits.x,w.limits.y);
            let q=(c-front)/width;
            if abs(q)>2.0 { continue; }
            // Compact crest/trough profile, zero height and derivative at its boundary.
            let s=max(1.0-q*q*0.25,0.0);
            let shape=s*s*s*(1.0-1.75*q*q);
            let fade=smoothstep(0.0,w.appearance.x,age)*(1.0-smoothstep(w.direction_birth_life.w-w.appearance.y,w.direction_birth_life.w,age));
            var decay=1.0;
            if w.appearance.z>0.0 { decay=exp2(-age/w.appearance.z); }
            height+=w.propagation.w*envelope*shape*fade*decay;
        }
    }
    height*=smoothstep(0.0,0.12,-data.x)*smoothstep(0.0,0.0005,grad)*smoothstep(0.0,lake.grid.y*0.02,length(world));
    height+=bounded_fft_height(world,data.x,data.w);
    // Bound the combined procedural + FFT sum, not each component.
    height=max(height,min(data.x*0.7,0.0));
    textureStore(heights,p,vec4(height,0.0,0.0,0.0));
}
fn h(p:vec2<i32>) -> f32 { return textureLoad(height_input,clamp(p,vec2(0),vec2(i32(lake.grid.x)-1)),0).x; }
@compute @workgroup_size(8,8)
fn normal_main(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=vec2<u32>(u32(lake.grid.x))) { return; }
    let p=vec2<i32>(id.xy);let spacing=lake.grid.y/(lake.grid.x-1.0);
    let slope=vec2(h(p+vec2(1,0))-h(p-vec2(1,0)),h(p+vec2(0,1))-h(p-vec2(0,1)))/(2.0*spacing);
    textureStore(field,p,vec4(h(p),slope,0.0));
}
