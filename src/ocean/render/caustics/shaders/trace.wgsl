@group(0) @binding(8) var<storage, read> height_ranges: array<vec2<f32>>;
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> fields: array<vec2<f32>>;
@group(0) @binding(2) var<uniform> optics: Optics;
@group(0) @binding(3) var<storage, read> shore_field: array<vec4<f32>>;
@group(0) @binding(4) var<uniform> shore: ShoreParams;
@group(0) @binding(5) var<storage, read> terrain_bed: array<vec2<f32>>;
@group(0) @binding(6) var<storage, read> shore_lace: array<f32>;
struct LightHit { origin: vec4<f32>, hit: vec4<f32>, transmission: vec4<f32> }
@group(0) @binding(7) var<storage, read_write> hits: array<LightHit>;

@compute @workgroup_size(8,8)
fn trace(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = u32(optics.refraction.w);
    if id.x > n || id.y > n { return; }
    let sun = normalize(camera.sun.xyz);
    let flat_ray = refract(-sun,vec3(0.0,1.0,0.0),1.0/optics.refraction.y);
    let center = caustics_center();
    let center_depth = max(-water_bed_height(center),0.0);
    // Guard band and upstream translation supply rays entering the receiver tile.
    let base = center + (vec2<f32>(id.xy)/f32(n)-vec2(0.5))*optics.caustics.z*1.5
        - flat_ray.xz*center_depth/max(-flat_ray.y,0.001);
    let grid = (base/camera.ocean.y+vec2(0.5))*camera.ocean.x;
    let p = surface(grid,vec2(0.0));
    let delta = max(0.02,min(1.0,optics.caustics.z*1.5/f32(n)*camera.ocean.x/camera.ocean.y));
    let tx = surface(grid+vec2(delta,0.0),vec2(0.0))-surface(grid-vec2(delta,0.0),vec2(0.0));
    let tz = surface(grid+vec2(0.0,delta),vec2(0.0))-surface(grid-vec2(0.0,delta),vec2(0.0));
    let area = cross(tz,tx);
    var normal = vec3(0.0,1.0,0.0);
    if dot(area,area)>1e-12 { normal=normalize(area); }
    let depth = p.y-water_bed_height(p.xz);
    let footprint=optics.caustics.z*1.5/f32(n)*camera.ocean.x/camera.ocean.y;
    normal=shore_detail_normal(normal,grid,p,depth,footprint);
    let ray = refract(-sun,normal,1.0/optics.refraction.y);
    var distance = water_bed_intersection(p,ray);
    var visits=0u;
    if shore.model.x>0.5 {
        let hit=trace_heightfield(p,ray,1000.0);
        let plane=p+ray*max(distance,0.0);
        // The background plane is valid only outside the modeled domain.
        if all(plane.xz>=shore.domain.xy) && all(plane.xz<=shore.domain.xy+shore.domain.zw) { distance=-1.0; }
        if hit.status==1u && (distance<0.0 || hit.distance<distance) { distance=hit.distance; }
        if hit.status==2u { distance=-1.0; }
        visits=hit.visits;
    }
    var valid = depth>0.008 && distance>0.0 && distance<1000.0 && sun.y>0.0 && dot(normal,sun)>0.0;
    let history=sample_shore_history(p.xz);
    if shore_active(shore) && ocean_weight(p.xz,shore)<0.01 && history.z<0.008 { valid=false; }
    // Foam is an optical coverage approximation, never a clamped simulation state.
    let foam_transmission=exp(-max(history.x,0.0)*shore.foam_material.w);
    let transmission=(1.0-water_fresnel(dot(normal,sun)))*exp(-optics.sigma_depth.xyz*max(distance,0.0))*foam_transmission;
    hits[id.y*(n+1u)+id.x]=LightHit(vec4(p,select(0.0,1.0,valid)),vec4(p+ray*max(distance,0.0),distance),vec4(transmission,f32(visits)));
}
