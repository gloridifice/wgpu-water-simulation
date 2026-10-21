// Shared linear-radiance sky background. The sun disc/halo is deliberately
// separate: it must not also enter the future direct-sun volume source.
fn sky_background(direction: vec3<f32>, tint_intensity: vec4<f32>) -> vec3<f32> {
    let elevation = pow(clamp(direction.y, 0.0, 1.0), 0.45);
    return mix(vec3(0.64, 0.79, 0.85), vec3(0.09, 0.30, 0.57), elevation)
        * tint_intensity.xyz * tint_intensity.w;
}
