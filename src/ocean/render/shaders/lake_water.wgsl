// Bounded geometry/mask adapter; all water optics use water::material.
@vertex
fn lake_vertex(@builtin(vertex_index) index:u32) -> WaterOut {
    let xz=lake_position(index);let f=lake_sample(lake_field,xz);
    let p=vec3(xz.x,f.x,xz.y);let normal=normalize(vec3(-f.y,1.0,-f.z));
    return WaterOut(camera.view_projection*vec4(p,1.0),p,normal,vec2(0.0),normal,2u,xz);
}
fn lake_water(input:WaterOut) -> vec4<f32> {
    let terrain=lake_sample(lake_terrain,input.world.xz);
    if terrain.x>=-0.003 { discard; }
    let normal=normalize(input.normal);
    let lighting=shade_water(input.world,normal,water_path(input,normal,false),vec3(0.0));
    var color=lighting.color;
    if lake.options.z>0.5 {
        let c=select(terrain.y,terrain.z,lake.options.y>0.5);
        if lake.options.z<1.5 { color=vec3(terrain.y); }
        else if lake.options.z<2.5 { color=vec3(0.1,0.5,0.9)*clamp(terrain.w/10.0,0.0,1.0); }
        else if lake.options.z<3.5 { color=mix(vec3(0.04,0.16,0.3),vec3(0.8,0.95,1.0),smoothstep(0.35,0.48,abs(fract(c*20.0)-0.5))); }
        else { color=vec3(0.5)+vec3(input.world.y*4.0); }
    }
    return vec4(color,1.0);
}
