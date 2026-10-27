struct Camera {
    view_projection: mat4x4<f32>, inverse_view_projection: mat4x4<f32>,
    eye: vec4<f32>, sun: vec4<f32>, ocean: vec4<f32>, misc: vec4<f32>,
}
struct ModelParams {
    transform: mat4x4<f32>, domain: vec4<f32>, low: vec4<f32>, high: vec4<f32>,
    options: vec4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> model: ModelParams;
@group(0) @binding(2) var color_map: texture_2d<f32>;
@group(0) @binding(3) var color_sampler: sampler;
@group(0) @binding(4) var<storage, read> history: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read> terrain: array<vec2<f32>>;
struct ModelOut {
    @builtin(position) position: vec4<f32>, @location(0) world: vec3<f32>, @location(1) uv: vec2<f32>,
}
@vertex fn model_vertex(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>, @location(2) uv: vec2<f32>) -> ModelOut {
    let world=model.transform*vec4(position,1.0);
    return ModelOut(camera.view_projection*world,world.xyz,uv);
}
@fragment fn model_fragment(input: ModelOut) -> @location(0) vec4<f32> {
    // This photogrammetry material is KHR_materials_unlit. Do not light its
    // photographed shadows a second time. Water optics is applied by the water pass.
    var color=textureSample(color_map,color_sampler,input.uv).rgb;
    let grid=(input.world.xz-model.domain.xy)/model.domain.zw*model.options.x;
    if model.options.y>0.5 && all(grid>=vec2(0.0)) && all(grid<vec2(model.options.x)) {
        let n=u32(model.options.x); let q=vec2<u32>(grid); let i=q.y*n+q.x;
        let bed=terrain[i].x;
        // An XZ history is not a water mark for an entire vertical cliff.
        let tolerance=max(0.05,0.5*max(model.domain.z,model.domain.w)/model.options.x);
        let match_bed=1.0-smoothstep(tolerance,2.0*tolerance,abs(input.world.y-bed));
        let wetness=history[i].y*match_bed;
        color*=mix(1.0,0.7,wetness);
        if model.options.w>3.5 && model.options.w<4.5 { color=mix(color,vec3(wetness),match_bed); }
    }
    return vec4(color,1.0);
}
struct LineOut { @builtin(position) position: vec4<f32> }
@vertex fn box_vertex(@builtin(vertex_index) index: u32) -> LineOut {
    let pairs=array<vec2<u32>,12>(vec2(0u,1u),vec2(2u,3u),vec2(4u,5u),vec2(6u,7u),
        vec2(0u,2u),vec2(1u,3u),vec2(4u,6u),vec2(5u,7u),
        vec2(0u,4u),vec2(1u,5u),vec2(2u,6u),vec2(3u,7u));
    let corner=pairs[index/2u][index%2u];
    let fraction=vec3(f32(corner&1u),f32((corner>>1u)&1u),f32((corner>>2u)&1u));
    return LineOut(camera.view_projection*vec4(mix(model.low.xyz,model.high.xyz,fraction),1.0));
}
@fragment fn box_fragment() -> @location(0) vec4<f32> { return vec4(0.95,0.55,0.08,1.0); }
