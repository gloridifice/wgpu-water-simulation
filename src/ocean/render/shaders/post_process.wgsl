// Linear HDR throughout. The final sRGB attachment performs the only encoding.
struct Settings {
    // threshold, soft-knee fraction, bloom strength, exposure multiplier
    bloom: vec4<f32>,
    // filter radius, enabled, unused, unused
    options: vec4<f32>,
}
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var low: texture_2d<f32>;
@group(0) @binding(2) var linear_clamp: sampler;
@group(0) @binding(3) var<uniform> settings: Settings;

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}
@vertex fn fullscreen(@builtin(vertex_index) index: u32) -> Vertex {
    let p = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return Vertex(vec4(p * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0), p);
}

fn sample_source(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(source, linear_clamp, uv, 0.0).rgb;
}
fn exposed(color: vec3<f32>) -> vec3<f32> {
    // Keep half-float pyramid writes finite even for extreme exposure/highlights.
    return min(max(color, vec3(0.0)), vec3(60000.0)) * settings.bloom.w;
}
fn bright(color: vec3<f32>) -> vec3<f32> {
    let c = min(exposed(color), vec3(60000.0));
    let peak = max(c.r, max(c.g, c.b));
    let knee = settings.bloom.x * settings.bloom.y;
    let transition = clamp(peak - settings.bloom.x + knee, 0.0, 2.0 * knee);
    let soft = transition * transition / max(4.0 * knee, 0.00001);
    let weight = max(peak - settings.bloom.x, soft) / max(peak, 0.00001);
    return c * weight;
}
@fragment fn prefilter(input: Vertex) -> @location(0) vec4<f32> {
    let d = 0.5 / vec2<f32>(textureDimensions(source));
    // Extract each sample before averaging so small HDR highlights survive.
    let c = bright(sample_source(input.uv + vec2(-d.x, -d.y)))
          + bright(sample_source(input.uv + vec2( d.x, -d.y)))
          + bright(sample_source(input.uv + vec2(-d.x,  d.y)))
          + bright(sample_source(input.uv + vec2( d.x,  d.y)));
    return vec4(c * 0.25, 1.0);
}
@fragment fn downsample(input: Vertex) -> @location(0) vec4<f32> {
    let d = 0.5 / vec2<f32>(textureDimensions(source));
    let c = sample_source(input.uv + vec2(-d.x, -d.y))
          + sample_source(input.uv + vec2( d.x, -d.y))
          + sample_source(input.uv + vec2(-d.x,  d.y))
          + sample_source(input.uv + vec2( d.x,  d.y));
    return vec4(c * 0.25, 1.0);
}
fn tent(uv: vec2<f32>) -> vec3<f32> {
    let d = settings.options.x / vec2<f32>(textureDimensions(low));
    var c = vec3(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let weight = f32((2 - abs(x)) * (2 - abs(y)));
            c += textureSampleLevel(low, linear_clamp, uv + vec2(f32(x), f32(y)) * d, 0.0).rgb * weight;
        }
    }
    return c / 16.0;
}
@fragment fn upsample(input: Vertex) -> @location(0) vec4<f32> {
    // Normalized accumulation keeps a uniform field independent of level count.
    return vec4(0.5 * (sample_source(input.uv) + tent(input.uv)), 1.0);
}
fn filmic(color: vec3<f32>) -> vec3<f32> {
    // ACES fitted curve, SDR output; not an HDR-display transfer function.
    let c = max(color, vec3(0.0));
    return clamp((c * (2.51 * c + vec3(0.03))) /
                 (c * (2.43 * c + vec3(0.59)) + vec3(0.14)), vec3(0.0), vec3(1.0));
}
@fragment fn composite(input: Vertex) -> @location(0) vec4<f32> {
    // Exact texel read avoids a second filtering of the main scene.
    let scene = textureLoad(source, vec2<i32>(input.position.xy), 0).rgb;
    if settings.options.y < 0.5 {
        return vec4(scene, 1.0);
    }
    var color = exposed(scene);
    if settings.bloom.z > 0.0 {
        color += settings.bloom.z * textureSampleLevel(low, linear_clamp, input.uv, 0.0).rgb;
    }
    return vec4(filmic(color), 1.0);
}
