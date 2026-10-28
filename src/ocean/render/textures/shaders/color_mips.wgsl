// Sampling an sRGB view decodes to linear; the sRGB attachment encodes on write.
@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(positions[i], 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let last = vec2<i32>(textureDimensions(source)) - vec2(1);
    let p = vec2<i32>(position.xy) * 2;
    // Clamping duplicates samples when one dimension has already reached 1.
    return 0.25 * (
        textureLoad(source, min(p, last), 0)
        + textureLoad(source, min(p + vec2(1, 0), last), 0)
        + textureLoad(source, min(p + vec2(0, 1), last), 0)
        + textureLoad(source, min(p + vec2(1, 1), last), 0));
}
