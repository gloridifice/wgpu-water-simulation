// Diagnostic entry only: freeze the prescribed water velocity and isolate the
// production transport/source/texture routines from shore_compute.wgsl.
// No SWE, FFT inlet, new foam, decay or edits to the production solver.
@compute @workgroup_size(8,8)
fn lab_step(@builtin(global_invocation_id) id: vec3<u32>) {
    let n=u32(shore.options.z);
    if id.x>=n || id.y>=n { return; }
    let cells=n*n; let i=id.y*n+id.x; let c=vec2<i32>(id.xy);
    let original=initial_shore[cells+i];
    var foam=transport_foam(c);
    if shore.solver.y>1.5 {
        foam=0.5*(foam+vec3(original.w,initial_shore[3u*cells+i].xy));
    }
    let predictor=shore.solver.y>0.5 && shore.solver.y<1.5;
    if !predictor { foam=foam_sources(foam,velocity(original),0.0); }
    let q=vec4(original.xyz,foam.x);
    new_shore[i]=vec4(foam.x,1.0,q.x,velocity(q).x);
    new_shore[cells+i]=q;
    new_shore[2u*cells+i]=initial_shore[2u*cells+i];
    if predictor {
        new_shore[3u*cells+i]=vec4(foam.yz,initial_shore[3u*cells+i].zw);
        new_shore[4u*cells+i]=initial_shore[4u*cells+i];
    } else {
        let motion=advance_foam_motion(c,q,original,foam.yz);
        new_shore[3u*cells+i]=motion.motion;
        new_shore[4u*cells+i]=motion.texture;
    }
}
