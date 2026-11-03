@group(0) @binding(4) var<storage, read> terrain_bed: array<vec2<f32>>;
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> optics: Optics;
@group(0) @binding(2) var<uniform> shore: ShoreParams;
struct LightHit { origin: vec4<f32>, hit: vec4<f32>, transmission: vec4<f32> }
@group(0) @binding(3) var<storage, read> hits: array<LightHit>;
struct CausticVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) irradiance: vec3<f32>,
}
@vertex fn vertex(@builtin(vertex_index) index: u32) -> CausticVertex {
    let n=u32(optics.refraction.w);
    let cell=index/6u;
    let corner=index%6u;
    let a=(cell/n)*(n+1u)+cell%n;
    var indices=vec3(a,a+n+1u,a+1u);
    if corner>=3u { indices=vec3(a+1u,a+n+1u,a+n+2u); }
    let p=hits[indices.x]; let q=hits[indices.y]; let r=hits[indices.z];
    let projected=abs(dot(cross(q.origin.xyz-p.origin.xyz,r.origin.xyz-p.origin.xyz),normalize(camera.sun.xyz)));
    let received=length(cross(q.hit.xyz-p.hit.xyz,r.hit.xyz-p.hit.xyz));
    // Finite receiver footprint bounds singular peaks. This is a regularization,
    // not exact transport at unresolved folds; resolution convergence is checked.
    let footprint=optics.caustics.z/512.0;
    let gain=projected/max(received,footprint*footprint*0.25);
    let uv=caustics_uv(hits[indices[corner%3u]].hit.xz);
    var valid=p.origin.w*q.origin.w*r.origin.w;
    if shore.model.x>0.5 {
        let center=(p.hit.xyz+q.hit.xyz+r.hit.xyz)/3.0;
        let grid_size=max(shore.domain.z,shore.domain.w)/shore.options.z;
        // Don't bridge ledges with a triangle whose interior leaves the receiver.
        if abs(model_terrain(center.xz,shore).x-center.y)>max(0.1,grid_size*1.5) { valid=0.0; }
    }
    var position=vec4(uv.x*2.0-1.0,1.0-uv.y*2.0,0.0,1.0);
    if valid<0.5 { position=vec4(2.0,2.0,0.0,1.0); }
    return CausticVertex(position,(p.transmission.xyz+q.transmission.xyz+r.transmission.xyz)*(gain/3.0));
}
@fragment fn fragment(input: CausticVertex) -> @location(0) vec4<f32> {
    return vec4(input.irradiance,0.0);
}
