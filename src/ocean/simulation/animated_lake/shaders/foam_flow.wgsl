// Render-only material-coordinate transport through a stationary synthetic flow.
// Same convention as SWE: publish displacement in metres, sample at world - offset.
// Integrating each layer from its cycle origin avoids frame partition dependence,
// accumulated interpolation diffusion and per-frame GPU history/readback.
@group(0) @binding(7) var foam_motion:texture_storage_2d<rgba32float,write>;
fn foam_shore_distance(world:vec2<f32>) -> f32 {
    let grid=clamp((world/lake.grid.y+0.5)*(lake.grid.x-1.0),vec2(0.0),vec2(lake.grid.x-1.001));
    let c=vec2<i32>(floor(grid));let f=fract(grid);
    return mix(mix(textureLoad(terrain,c,0).w,textureLoad(terrain,c+vec2(1,0),0).w,f.x),
               mix(textureLoad(terrain,c+vec2(0,1),0).w,textureLoad(terrain,c+vec2(1,1),0).w,f.x),f.y);
}
fn foam_flow_velocity(world:vec2<f32>, distance:f32) -> vec2<f32> {
    let direction=-world/sqrt(dot(world,world)+1.0);
    // A nearly constant local velocity only translates the net. Slower flow beside
    // solids and faster flow in open water stretches material away from the shore
    // and shears it beside rocks, while retaining the requested inward direction.
    let ramp=max(lake.foam.z,2.0*lake.grid.y/(lake.grid.x-1.0));
    let gain=mix(1.0,smoothstep(0.0,ramp,max(distance,0.0)),lake.foam_flow.w);
    return direction*lake.foam_flow.x*gain;
}
fn foam_flow_offset(world:vec2<f32>, age:f32) -> vec2<f32> {
    var departure=world;
    // Bound distance per RK2 step relative to the speed-gradient length. Both
    // phases use the same integration step size; only their final step differs.
    let step_time=max(0.1,lake.foam.z*0.12)/max(lake.foam_flow.x,0.001);
    var remaining=age;
    for(var i=0u;i<512u;i++) {
        if remaining<=0.0 { break; }
        let dt=min(remaining,step_time);
        let distance=max(foam_shore_distance(departure),0.0);
        let v=foam_flow_velocity(departure,distance);
        let midpoint=departure-v*(0.5*dt);
        let mid_distance=foam_shore_distance(midpoint);
        var shift=foam_flow_velocity(midpoint,mid_distance)*dt;
        // Never trace through an island or off the wet domain. Unlike SWE there
        // is no wet-donor history to interpolate: the stationary SDF bounds steps.
        shift*=min(1.0,0.8*distance/max(length(shift),0.000001));
        departure-=shift;
        remaining-=dt;
    }
    return world-departure;
}
@compute @workgroup_size(8,8)
fn foam_flow_main(@builtin(global_invocation_id) id:vec3<u32>) {
    if any(id.xy>=vec2<u32>(u32(lake.grid.x))) { return; }
    let c=vec2<i32>(id.xy);
    if lake.foam.x<=0.0 || lake.foam_flow.x<=0.0 || textureLoad(terrain,c,0).x>=0.0 {
        textureStore(foam_motion,c,vec4(0.0));return;
    }
    let world=(vec2<f32>(id.xy)/(lake.grid.x-1.0)-0.5)*lake.grid.y;
    let age_a=lake.foam_flow.z*lake.foam_flow.y;
    let age_b=fract(lake.foam_flow.z+0.5)*lake.foam_flow.y;
    textureStore(foam_motion,c,vec4(foam_flow_offset(world,age_a),foam_flow_offset(world,age_b)));
}
