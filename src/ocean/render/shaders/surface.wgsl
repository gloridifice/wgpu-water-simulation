// 周期双线性采样，FFT 网格与渲染网格独立。
fn field_at(grid: vec2<i32>, channel: u32) -> f32 {
    let n = i32(camera.ocean.x);
    let wrapped = ((grid % vec2(n)) + vec2(n)) % vec2(n);
    return fields[channel * u32(n * n) + u32(wrapped.y * n + wrapped.x)].x;
}
fn sample_field(grid: vec2<f32>, channel: u32) -> f32 {
    let cell = vec2<i32>(floor(grid));
    let f = fract(grid);
    return mix(mix(field_at(cell, channel), field_at(cell + vec2(1, 0), channel), f.x),
               mix(field_at(cell + vec2(0, 1), channel), field_at(cell + vec2(1, 1), channel), f.x), f.y);
}
fn surface(grid: vec2<f32>, tile_offset: vec2<f32>) -> vec3<f32> {
    let n = camera.ocean.x;
    let base = (grid / n - vec2(0.5)) * camera.ocean.y + tile_offset;
    let weight = ocean_weight(base, shore);
    let h = sample_field(grid, 0u) * camera.ocean.w * weight + shore_sample(base, shore).height;
    let dx = sample_field(grid, 1u) * camera.ocean.z * camera.ocean.w * weight;
    let dz = sample_field(grid, 2u) * camera.ocean.z * camera.ocean.w * weight;
    return vec3(base.x + dx, h, base.y + dz);
}

// Render-only high-frequency slope at an FFT node: keep the existing one-cell
// minus four-cell centered differences. Metric spacing and height gain are
// applied once after interpolation; neither SWE input nor geometry is changed.
fn fft_detail_slope_at(grid: vec2<i32>) -> vec2<f32> {
    let dx = field_at(grid + vec2(1, 0), 0u) - field_at(grid - vec2(1, 0), 0u)
        - 0.25 * (field_at(grid + vec2(4, 0), 0u) - field_at(grid - vec2(4, 0), 0u));
    let dz = field_at(grid + vec2(0, 1), 0u) - field_at(grid - vec2(0, 1), 0u)
        - 0.25 * (field_at(grid + vec2(0, 4), 0u) - field_at(grid - vec2(0, 4), 0u));
    return vec2(dx, dz);
}
// Catmull-Rom preserves nodal slopes and shares first derivatives across cells.
// Bilinear slopes were continuous but their derivative jumps outlined FFT cells
// in nearshore highlights. Do not change sample_field(): it also drives geometry.
fn detail_cubic_weights(t: f32) -> vec4<f32> {
    let t2 = t * t;
    let t3 = t2 * t;
    return vec4(-0.5 * t + t2 - 0.5 * t3,
        1.0 - 2.5 * t2 + 1.5 * t3,
        0.5 * t + 2.0 * t2 - 1.5 * t3,
        -0.5 * t2 + 0.5 * t3);
}
fn fft_detail_slope(grid: vec2<f32>) -> vec2<f32> {
    let cell = vec2<i32>(floor(grid));
    let wx = detail_cubic_weights(fract(grid.x));
    let wz = detail_cubic_weights(fract(grid.y));
    var slope = vec2(0.0);
    for (var z = 0; z < 4; z++) {
        var row = vec2(0.0);
        for (var x = 0; x < 4; x++) {
            row += wx[x] * fft_detail_slope_at(cell + vec2(x - 1, z - 1));
        }
        slope += wz[z] * row;
    }
    return slope * camera.ocean.w * camera.ocean.x / (2.0 * camera.ocean.y);
}
fn shore_detail_slope(grid: vec2<f32>, world: vec3<f32>, depth: f32, footprint: f32) -> vec2<f32> {
    if !shore_active(shore) || shore.visual.x <= 0.0 { return vec2(0.0); }
    let weight = (1.0 - ocean_weight(world.xz, shore)) * smoothstep(0.02, 0.25, depth)
        * (1.0 - smoothstep(0.5, 2.0, footprint)) * shore.visual.x;
    if weight <= 0.0 { return vec2(0.0); }
    return fft_detail_slope(grid) * weight;
}
fn apply_detail_slope(base: vec3<f32>, slope: vec2<f32>) -> vec3<f32> {
    if all(slope == vec2(0.0)) { return base; }
    return normalize(vec3(base.x - base.y * slope.x, base.y, base.z - base.y * slope.y));
}
// Light-ray consumers share the same reconstruction with their own footprint.
fn shore_detail_normal(base: vec3<f32>, grid: vec2<f32>, world: vec3<f32>, depth: f32, footprint: f32) -> vec3<f32> {
    return apply_detail_slope(base, shore_detail_slope(grid, world, depth, footprint));
}
