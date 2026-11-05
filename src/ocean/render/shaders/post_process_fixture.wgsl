// HDR highlight fixture for the headless post-processing verification.
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(p * 2.0 - vec2(1.0), 0.0, 1.0);
}
@fragment fn fragment() -> @location(0) vec4<f32> {
    return vec4(8.0, 4.0, 2.0, 1.0);
}
