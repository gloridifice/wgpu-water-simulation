@group(0) @binding(0) var<uniform> camera:Camera;
@group(0) @binding(1) var<uniform> optics:Optics;
struct GroundOut { @builtin(position) position:vec4<f32>, @location(0) world:vec3<f32>, @location(1) normal:vec3<f32>, @location(2) curvature:f32 }
@vertex
fn ground_vertex(@builtin(vertex_index) index:u32) -> GroundOut {
    let xz=lake_position(index);let step=lake.grid.y/(lake.grid.x-1.0);
    let bed=lake_sample(lake_terrain,xz).x;
    let dx=lake_sample(lake_terrain,xz+vec2(step,0.0)).x-lake_sample(lake_terrain,xz-vec2(step,0.0)).x;
    let dz=lake_sample(lake_terrain,xz+vec2(0.0,step)).x-lake_sample(lake_terrain,xz-vec2(0.0,step)).x;
    let world=vec3(xz.x,bed,xz.y);
    let laplacian=(lake_sample(lake_terrain,xz+vec2(step,0.0)).x+lake_sample(lake_terrain,xz-vec2(step,0.0)).x
        +lake_sample(lake_terrain,xz+vec2(0.0,step)).x+lake_sample(lake_terrain,xz-vec2(0.0,step)).x-4.0*bed)/(step*step);
    return GroundOut(camera.view_projection*vec4(world,1.0),world,normalize(vec3(-dx,2.0*step,-dz)),-laplacian);
}
@fragment
fn ground_fragment(input:GroundOut) -> @location(0) vec4<f32> {
    let normal=normalize(input.normal);
    let detail=0.92+0.08*sin(input.world.x*11.0+sin(input.world.z*7.0))*cos(input.world.z*13.0);
    let grass=mix(vec3(0.24,0.22,0.13),vec3(0.12,0.20,0.055),smoothstep(0.1,1.2,input.world.y));
    let rock=max(smoothstep(0.15,0.65,1.0-normal.y),smoothstep(0.1,0.5,input.curvature));
    var base=mix(grass,vec3(0.27,0.29,0.28),rock)*detail;
    base*=0.45+0.55*max(dot(normal,camera.sun.xyz),0.0);
    if lake.options.z>1.5 && lake.options.z<2.5 {
        let d=lake_sample(lake_terrain,input.world.xz).w;
        base=select(vec3(0.8,0.15,0.05),vec3(0.1,0.5,0.9),d>0.0)*min(abs(d)/10.0,1.0);
    }
    return vec4(base,1.0);
}
