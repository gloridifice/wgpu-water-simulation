// Shared bathymetry and local SWE configuration.
struct Collider { footprint: vec4<f32>, solid: vec4<f32> }
struct ShoreParams {
    terrain: vec4<f32>, // enabled, slope, shoreline X, flat seabed depth
    waves: vec4<f32>,   // spectral height bound, startup age, blend depth, enabled
    domain: vec4<f32>,  // origin X/Z, extent X/Z
    timing: vec4<f32>,  // time, dt, foam decay /s, wetness drying seconds
    coupling: vec4<f32>, // FFT resolution, tile length, height gain, time interpolation
    filter_settings: vec4<f32>, // enabled, rejected wavelength, transition width, padding
    solver: vec4<f32>, // order (1/2), RK stage (0/1/2), render height gain, SWE input gain
    visual: vec4<f32>, // render only: normal strength, foam max lift (m), density threshold, transition
    density: vec4<f32>, // compression threshold /s, display reference (amount/area), minimum foam water depth (m), reserved
    foam_generation: vec4<f32>, // normalized rise threshold, transition width, reserved
    foam_front: vec4<f32>, // front rate /s, sample distance m, reserved, background multiplier
    foam_velocity: vec4<f32>, // reserved, threshold /s, transition /s, reserved
    foam_motion: vec4<f32>, // flow following multiplier, response time seconds, reserved
    foam_inertia: vec4<f32>, // drag enabled, newborn water velocity multiplier, reserved
    water_foam_color: vec4<f32>, // linear RGB, render only
    foam_material: vec4<f32>, // source (0 legacy, 1..3 NSH), tile size m, micro normal, opacity density scale
    foam_shading: vec4<f32>, // reserved, roughness, material debug, reserved
    options: vec4<f32>, // foam generation /s, debug view, resolution, reset
    collider_meta: vec4<f32>, // packed active count, reserved
    model: vec4<f32>, // enabled, minimum bed, fallback bed, sponge width
    edges: vec4<f32>, // open boundaries: -X,+X,-Z,+Z
    colliders: array<Collider, 8>,
}

// Density is not clipped in the solver. This reference sets the debug color
// scale and pore coverage, never the NSH optical opacity response.
fn foam_density_level(amount: f32, s: ShoreParams) -> f32 {
    return clamp(amount/s.density.y,0.0,1.0);
}
fn foam_density_color(amount: f32, s: ShoreParams) -> vec3<f32> {
    let level=foam_density_level(amount,s);
    return mix(vec3(0.0,1.0,0.0),vec3(1.0,0.0,0.0),level);
}

fn shore_distance(p: vec2<f32>, s: ShoreParams) -> f32 {
    return p.x - s.terrain.z;
}
// Each consumer declares the same static height/coverage buffer at its own binding.
fn terrain_cell(p: vec2<i32>, s: ShoreParams) -> vec2<f32> {
    let n=i32(s.options.z); let q=clamp(p,vec2(0),vec2(n-1));
    return terrain_bed[u32(q.y*n+q.x)];
}
fn model_terrain(p: vec2<f32>, s: ShoreParams) -> vec2<f32> {
    if any(p<s.domain.xy) || any(p>s.domain.xy+s.domain.zw) { return vec2(s.model.z,0.0); }
    let grid=(p-s.domain.xy)/s.domain.zw*s.options.z-vec2(0.5);
    let q=vec2<i32>(floor(grid)); let f=fract(grid);
    return mix(mix(terrain_cell(q,s),terrain_cell(q+vec2(1,0),s),f.x),
        mix(terrain_cell(q+vec2(0,1),s),terrain_cell(q+vec2(1,1),s),f.x),f.y);
}
fn model_edge_weight(p: vec2<f32>, s: ShoreParams) -> f32 {
    let d=vec4(p.x-s.domain.x,s.domain.x+s.domain.z-p.x,p.y-s.domain.y,s.domain.y+s.domain.w-p.y);
    let weights=(vec4(1.0)-smoothstep(vec4(0.0),vec4(s.model.w),d))*s.edges;
    return max(max(weights.x,weights.y),max(weights.z,weights.w));
}
fn terrain_height(p: vec2<f32>, s: ShoreParams) -> f32 {
    if s.model.x>0.5 { return model_terrain(p,s).x; }
    if s.terrain.x < 0.5 { return -s.terrain.w; }
    return -s.terrain.y * shore_distance(p, s);
}
fn collider_local(p: vec2<f32>, c: Collider) -> vec2<f32> {
    let d=p-c.footprint.xy;
    return vec2(c.solid.z*d.x+c.solid.w*d.y,-c.solid.w*d.x+c.solid.z*d.y);
}
fn collider_distance(p: vec2<f32>, c: Collider) -> f32 {
    let q=collider_local(p,c);
    if c.solid.x<1.5 { return length(q)-c.footprint.z; }
    let d=abs(q)-c.footprint.zw;
    return length(max(d,vec2(0.0)))+min(max(d.x,d.y),0.0);
}
fn bottom_height(p: vec2<f32>, s: ShoreParams) -> f32 {
    var height=terrain_height(p,s);
    if !shore_active(s) { return height; }
    for(var i=0u;i<u32(s.collider_meta.x);i++) {
        let c=s.colliders[i];
        // Avoid square roots in the frequently evaluated solver bed query.
        let q=collider_local(p,c);
        let inside=select(all(abs(q)<=c.footprint.zw),dot(q,q)<=c.footprint.z*c.footprint.z,c.solid.x<1.5);
        if inside { height=max(height,c.solid.y); }
    }
    return height;
}
fn shore_active(s: ShoreParams) -> bool {
    return (s.terrain.x > 0.5 || s.model.x > 0.5) && s.waves.w > 0.5;
}
fn ocean_weight(p: vec2<f32>, s: ShoreParams) -> f32 {
    if !shore_active(s) { return 1.0; }
    if s.model.x>0.5 {
        let d=min(p-s.domain.xy,s.domain.xy+s.domain.zw-p);
        let inside=smoothstep(0.0,2.0*max(s.domain.z,s.domain.w)/s.options.z,min(d.x,d.y));
        return max(1.0-inside,model_edge_weight(p,s));
    }
    let base_weight=smoothstep(s.waves.z, s.waves.z * 2.5, -terrain_height(p, s));
    if s.collider_meta.x<0.5 { return base_weight; }
    var weight=base_weight;
    // Show the SWE response locally, even when a collider is in the FFT blend.
    // The original FFT spectrum and the one-way input remain untouched.
    for(var i=0u;i<u32(s.collider_meta.x);i++) {
        weight*=smoothstep(2.0,8.0,collider_distance(p,s.colliders[i]));
    }
    let to_edge=min(p-s.domain.xy,s.domain.xy+s.domain.zw-p);
    let inside=smoothstep(0.0,2.0*max(s.domain.z,s.domain.w)/s.options.z,min(to_edge.x,to_edge.y));
    let inlet=1.0-smoothstep(0.78,0.80,(p.x-s.domain.x)/s.domain.z);
    return mix(base_weight,weight,inside*inlet);
}
struct ShoreSample {
    height: f32,
    velocity: vec2<f32>,
    breaking: f32,
}
// Non-periodic world-space noise shared by emission and foam edge erosion.
fn shore_noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (vec2(3.0) - 2.0 * f);
    let h = fract(sin(vec4(dot(cell, vec2(127.1, 311.7)),
        dot(cell + vec2(1, 0), vec2(127.1, 311.7)),
        dot(cell + vec2(0, 1), vec2(127.1, 311.7)),
        dot(cell + vec2(1, 1), vec2(127.1, 311.7)))) * 43758.5453);
    return mix(mix(h.x, h.y, u.x), mix(h.z, h.w, u.x), u.y);
}
