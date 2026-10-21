struct Params { n: u32, span: u32, axis: u32, bits: u32 }
@group(0) @binding(0) var<storage, read> source: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> destination: array<vec2<f32>>;
@group(0) @binding(2) var<uniform> params: Params;

fn read_index(channel: u32, x: u32, z: u32, coordinate: u32) -> u32 {
    var c = coordinate;
    // 每个轴的第一级先按位反转，随后执行迭代 radix-2 DIT 蝶形。
    if params.span == 2u { c = reverseBits(c) >> (32u - params.bits); }
    if params.axis == 0u { return channel * params.n * params.n + z * params.n + c; }
    return channel * params.n * params.n + c * params.n + x;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    let n = params.n;
    if index >= 6u * n * n { return; }
    let channel = index / (n * n);
    let x = index % n;
    let z = (index / n) % n;
    let coordinate = select(x, z, params.axis == 1u);
    let half_span = params.span / 2u;
    let j = coordinate % half_span;
    let start = (coordinate / params.span) * params.span;
    let a = source[read_index(channel, x, z, start + j)];
    let b = source[read_index(channel, x, z, start + j + half_span)];
    // 逆变换使用正号；每轴最后一级归一化一次。
    let angle = 6.28318530718 * f32(j) / f32(params.span);
    let w = vec2(cos(angle), sin(angle));
    let wb = vec2(w.x*b.x - w.y*b.y, w.x*b.y + w.y*b.x);
    let sign = select(1.0, -1.0, coordinate % params.span >= half_span);
    let scale = select(1.0, 1.0 / f32(n), params.span == n);
    destination[index] = (a + sign * wb) * scale;
}
