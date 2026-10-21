// Shared by the water fragment shader and the GPU numerical check.
struct MediumResult {
    transmittance: vec3<f32>,
    background: vec3<f32>,
    scattering: vec3<f32>,
}
fn optical_weight(sigma: f32, d: f32) -> f32 {
    let tau = sigma * d;
    if tau < 0.001 {
        return d * (1.0 - tau * 0.5 + tau * tau / 6.0 - tau * tau * tau / 24.0);
    }
    return (1.0 - exp(-tau)) / sigma;
}
fn integrate_medium(bg: vec3<f32>, sigma_t: vec3<f32>, source: vec3<f32>, distance: f32) -> MediumResult {
    let d = max(distance, 0.0);
    let tau = sigma_t * d;
    let t = exp(-tau);
    var weight: vec3<f32>;
    for (var channel = 0u; channel < 3u; channel++) {
        // Shared by constant manual S and each piecewise sky segment.
        weight[channel] = optical_weight(sigma_t[channel], d);
    }
    return MediumResult(t, bg * t, source * weight);
}
