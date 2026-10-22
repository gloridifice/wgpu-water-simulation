// Shared bathymetry and local SWE configuration.
struct ShoreParams {
    terrain: vec4<f32>, // enabled, slope, shoreline X, flat seabed depth
    waves: vec4<f32>,   // spectral height bound, startup age, blend depth, enabled
    domain: vec4<f32>,  // origin X/Z, extent X/Z
    timing: vec4<f32>,  // time, dt, foam decay /s, wetness drying seconds
    coupling: vec4<f32>, // FFT resolution, tile length, height gain, time interpolation
    filter_settings: vec4<f32>, // enabled, rejected wavelength, transition width, padding
    solver: vec4<f32>, // order (1/2), RK stage (0/1/2), render height gain, SWE input gain
    visual: vec4<f32>, // normal detail strength (render only), reserved
    density: vec4<f32>, // compression threshold /s, display reference (amount/area), foam deposit depth (m), reserved
    foam_generation: vec4<f32>, // normalized rise threshold, transition width, reserved
    foam_front: vec4<f32>, // front rate /s, sample distance m, full-weight surface drop m, background multiplier
    foam_motion: vec4<f32>, // flow following multiplier, response time seconds, reserved
    water_foam_color: vec4<f32>, // linear RGB, render only
    stranded_foam_color: vec4<f32>, // linear RGB, render only
    options: vec4<f32>, // foam generation /s, debug view, resolution, reset
}

// Density is not clipped in the solver; this reference only sets the color scale.
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
fn bottom_height(p: vec2<f32>, s: ShoreParams) -> f32 {
    if s.terrain.x < 0.5 { return -s.terrain.w; }
    return -s.terrain.y * shore_distance(p, s);
}
fn shore_active(s: ShoreParams) -> bool {
    return s.terrain.x > 0.5 && s.waves.w > 0.5;
}
fn ocean_weight(p: vec2<f32>, s: ShoreParams) -> f32 {
    if !shore_active(s) { return 1.0; }
    return smoothstep(s.waves.z, s.waves.z * 2.5, -bottom_height(p, s));
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
