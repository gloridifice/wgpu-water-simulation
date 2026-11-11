// Shared periodic bilinear sampling. The consumer supplies fields and fft_resolution().
fn field_at(grid: vec2<i32>, channel: u32) -> f32 {
    let n = i32(fft_resolution());
    let wrapped = ((grid % vec2(n)) + vec2(n)) % vec2(n);
    return fields[channel * u32(n * n) + u32(wrapped.y * n + wrapped.x)].x;
}
fn sample_field(grid: vec2<f32>, channel: u32) -> f32 {
    let cell = vec2<i32>(floor(grid));
    let f = fract(grid);
    return mix(mix(field_at(cell, channel), field_at(cell + vec2(1, 0), channel), f.x),
               mix(field_at(cell + vec2(0, 1), channel), field_at(cell + vec2(1, 1), channel), f.x), f.y);
}
