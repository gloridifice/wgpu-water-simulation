struct Camera {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    sun: vec4<f32>,
    ocean: vec4<f32>,
    misc: vec4<f32>,
}
struct Optics {
    sigma_depth: vec4<f32>,
    source_view: vec4<f32>,
    options: vec4<f32>,
    sky_color: vec4<f32>,
    sky_albedo_enabled: vec4<f32>,
    sky_quality: vec4<f32>,
    beach: vec4<f32>,
    sand_mapping: vec4<f32>, // cos(rotation), sin(rotation), reserved
    caustics: vec4<f32>, // enabled, direct-light strength, receiver width m, filter radius texels
    refraction: vec4<f32>, // 0 off/1 physical/2 fast, water IOR, max eye-ray length m, light grid cells
    fast_refraction: vec4<f32>, // strength, full-distortion depth m, viewport height fraction, reserved
    reflection: vec4<f32>, // SSR enabled, maximum reflected path m, strength, edge fade pixels
    reflection_trace: vec4<f32>, // SSR block-visit budget, tolerance multiplier, reserved
}
