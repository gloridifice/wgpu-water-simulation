@group(0) @binding(0) var<uniform> sky_tint: vec4<f32>;
@group(0) @binding(1) var output_lut: texture_storage_2d<rgba32float, write>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = textureDimensions(output_lut).x;
    if id.x >= n { return; }
    let u = f32(id.x) / f32(n - 1u);
    // Quadratic optical-depth spacing resolves the rapid change near the surface.
    let tau = 32.0 * u * u;
    var light = vec3(0.0);
    for (var i = 0u; i < 32u; i++) {
        // Equal-solid-angle hemisphere samples; current diffuse sky is axial.
        let mu = (f32(i) + 0.5) / 32.0;
        let direction = vec3(sqrt(1.0 - mu * mu), mu, 0.0);
        light += sky_background(direction, sky_tint) * exp(-tau / mu);
    }
    // dOmega=2pi/N, isotropic phase=1/(4pi). No surface cosine here.
    textureStore(output_lut, vec2<i32>(i32(id.x), 0), vec4(light * (0.5 / 32.0), 0.0));
}
