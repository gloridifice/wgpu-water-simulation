fn caustic_light(world: vec3<f32>, height: f32) -> vec3<f32> {
    let uv=caustics_uv(world.xz);
    let radius=optics.caustics.w/vec2<f32>(textureDimensions(caustic_map));
    var irradiance=textureSampleLevel(caustic_map,caustic_sampler,uv,0.0).rgb*0.5;
    irradiance+=textureSampleLevel(caustic_map,caustic_sampler,uv+vec2(radius.x,0.0),0.0).rgb*0.125;
    irradiance+=textureSampleLevel(caustic_map,caustic_sampler,uv-vec2(radius.x,0.0),0.0).rgb*0.125;
    irradiance+=textureSampleLevel(caustic_map,caustic_sampler,uv+vec2(0.0,radius.y),0.0).rgb*0.125;
    irradiance+=textureSampleLevel(caustic_map,caustic_sampler,uv-vec2(0.0,radius.y),0.0).rgb*0.125;
    let edge=min(min(uv.x,uv.y),min(1.0-uv.x,1.0-uv.y));
    return mix(flat_water_light(world,height),irradiance,smoothstep(0.0,0.08,edge));
}
