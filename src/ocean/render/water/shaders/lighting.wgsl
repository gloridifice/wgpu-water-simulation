// Shared environment/SSR and dielectric direct lighting. Scene adapters supply
// camera, optics, scene color/depth and the trace_screen implementation.
fn sky_with_halo(direction: vec3<f32>) -> vec3<f32> {
    let sun = max(dot(direction, camera.sun.xyz), 0.0);
    return sky_background(direction, optics.sky_color)
        + vec3(1.0, 0.75, 0.40) * (0.20 * pow(sun, 40.0));
}
// Cook–Torrance dielectric reflection under a directional sun: BRDF times N·L.
fn water_sun_response(normal: vec3<f32>, view: vec3<f32>, light: vec3<f32>) -> f32 {
    let no_v = clamp(dot(normal, view), 0.0, 1.0);
    let no_l = clamp(dot(normal, light), 0.0, 1.0);
    if no_v <= 0.0 || no_l <= 0.0 || light.y <= 0.0 { return 0.0; }
    let half_sum = view + light;
    let half_length_squared = dot(half_sum, half_sum);
    if half_length_squared < 1e-8 { return 0.0; }
    let half_vector = half_sum * inverseSqrt(half_length_squared);
    let no_h = clamp(dot(normal, half_vector), 0.0, 1.0);
    let lo_h = clamp(dot(light, half_vector), 0.0, 1.0);
    // Residual roughness below the scale of the resolved wave normal.
    let roughness = 0.28;
    let alpha = roughness * roughness;
    let alpha_squared = alpha * alpha;
    let no_h_squared = no_h * no_h;
    let ggx = (1.0 - no_h_squared) + alpha_squared * no_h_squared;
    let distribution = alpha_squared / (3.14159265 * ggx * ggx);
    // Height-correlated Smith visibility (includes the BRDF denominator).
    let shadow_view = no_l * sqrt(no_v * no_v * (1.0 - alpha_squared) + alpha_squared);
    let shadow_light = no_v * sqrt(no_l * no_l * (1.0 - alpha_squared) + alpha_squared);
    let visibility_times_cosine = 0.5 * no_l / (shadow_view + shadow_light);
    return distribution * visibility_times_cosine * water_fresnel(lo_h);
}
// RGB is the sampled scene radiance; alpha is SSR's contribution weight.
fn screen_reflection(origin:vec3<f32>, normal:vec3<f32>, incoming:vec3<f32>) -> vec4<f32> {
    if optics.reflection.x<0.5 || incoming.y>=0.0 || dot(normal,-incoming)<=0.0 { return vec4(0.0); }
    let maximum=optics.reflection.y;
    let hit=trace_screen(origin,reflect(incoming,normal),maximum,true);
    if !hit.valid { return vec4(0.0); }
    let size=vec2<f32>(textureDimensions(scene_depth));
    let pixel=vec2<f32>(hit.pixel)+vec2(0.5);
    let edge=min(min(pixel.x,pixel.y),min(size.x-pixel.x,size.y-pixel.y));
    let edge_weight=smoothstep(0.0,max(optics.reflection.w,0.0001),edge);
    let confidence=optics.reflection.z*edge_weight*(1.0-smoothstep(0.8*maximum,maximum,hit.distance));
    return vec4(textureLoad(scene_color,hit.pixel,0).rgb,confidence);
}
