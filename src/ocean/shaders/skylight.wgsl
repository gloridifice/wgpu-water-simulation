// Appended to the render shader; optics, sky_lut and optical_weight are shared.
fn sky_light_at(tau: f32, channel: u32) -> f32 {
    // Beyond 32 optical depths the omitted incident radiance is < 1.3e-14.
    if tau >= 32.0 { return 0.0; }
    let count = textureDimensions(sky_lut).x;
    let position = sqrt(max(tau, 0.0) / 32.0) * f32(count - 1u);
    let index = min(u32(position), count - 2u);
    let a = textureLoad(sky_lut, vec2<i32>(i32(index), 0), 0)[channel];
    let b = textureLoad(sky_lut, vec2<i32>(i32(index + 1u), 0), 0)[channel];
    // Interpolate in optical depth, not in the warped texture coordinate.
    let u0 = f32(index) / f32(count - 1u);
    let u1 = f32(index + 1u) / f32(count - 1u);
    let weight = (max(tau, 0.0) - 32.0 * u0 * u0) / (32.0 * (u1 * u1 - u0 * u0));
    return mix(a, b, clamp(weight, 0.0, 1.0));
}
fn sky_source_at(world: vec3<f32>) -> vec3<f32> {
    var result = vec3(0.0);
    if optics.sky_albedo_enabled.w < 0.5 { return result; }
    let depth = max(-world.y, 0.0); // Mean plane y=0; crests use surface lighting.
    for (var c = 0u; c < 3u; c++) {
        let sigma = optics.sigma_depth[c];
        result[c] = optics.sky_albedo_enabled[c] * sigma * sky_light_at(sigma * depth, c);
    }
    return result;
}
fn integrate_skylight(origin: vec3<f32>, direction: vec3<f32>, distance: f32) -> vec3<f32> {
    var result = vec3(0.0);
    if optics.sky_albedo_enabled.w < 0.5 { return result; }
    let steps = u32(optics.sky_quality.x);
    for (var c = 0u; c < 3u; c++) {
        let sigma = optics.sigma_depth[c];
        let sigma_s = sigma * optics.sky_albedo_enabled[c];
        if sigma_s <= 0.0 { continue; }
        // Per-channel range prevents a transparent blue channel from undersampling
        // the thin red scattering layer. The discarded view tail is <= exp(-16).
        let end = min(max(distance, 0.0), 16.0 / sigma);
        for (var i = 0u; i < steps; i++) {
            let u0 = f32(i) / f32(steps);
            let u1 = f32(i + 1u) / f32(steps);
            let start = end * u0 * u0;
            let finish = end * u1 * u1;
            let middle = origin + direction * ((start + finish) * 0.5);
            let source = sigma_s * sky_light_at(sigma * max(-middle.y, 0.0), c);
            result[c] += exp(-sigma * start) * source * optical_weight(sigma, finish - start);
        }
    }
    return result;
}
