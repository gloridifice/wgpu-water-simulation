struct LakeParams { grid:vec4<f32>, options:vec4<f32>, fft:vec4<f32> }
@group(1) @binding(0) var<uniform> lake:LakeParams;
@group(1) @binding(1) var lake_terrain:texture_2d<f32>;
@group(1) @binding(2) var lake_field:texture_2d<f32>;
fn lake_sample(map:texture_2d<f32>,world:vec2<f32>) -> vec4<f32> {
    let grid=clamp((world/lake.grid.y+vec2(0.5))*(lake.grid.x-1.0),vec2(0.0),vec2(lake.grid.x-1.001));
    let p=vec2<i32>(floor(grid));let f=fract(grid);
    return mix(mix(textureLoad(map,p,0),textureLoad(map,p+vec2(1,0),0),f.x),mix(textureLoad(map,p+vec2(0,1),0),textureLoad(map,p+vec2(1,1),0),f.x),f.y);
}
fn lake_position(index:u32) -> vec2<f32> {
    let n=u32(lake.grid.x);
    return (vec2<f32>(f32(index%n),f32(index/n))/(lake.grid.x-1.0)-vec2(0.5))*lake.grid.y;
}
