struct Params { n: u32, time: f32, length: f32, min_wavelength: f32 }
@group(0) @binding(0) var<storage, read> initial: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> output: array<vec2<f32>>;
@group(0) @binding(2) var<uniform> params: Params;

fn multiply(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = params.n;
    let index = id.x;
    if index >= n * n { return; }
    let x = index % n;
    let z = index / n;
    let opposite = ((n - z) % n) * n + (n - x) % n;
    let mode = initial[index];
    let k = length(mode.zw);
    let angle = sqrt(9.81 * k) * params.time;
    let phase = vec2(cos(angle), sin(angle));
    let other = initial[opposite].xy * vec2(1.0, -1.0);
    // H(k,t) = h0(k)e^(iwt) + conjugate(h0(-k))e^(-iwt).
    let a = multiply(mode.xy, phase);
    let b = multiply(other, phase * vec2(1.0, -1.0));
    let h = a + b;
    output[index] = h;
    // 水平位移 D(k) = -i * k/|k| * H(k)，三个通道共用 FFT。
    let minus_i_h = vec2(h.y, -h.x);
    output[n * n + index] = minus_i_h * mode.z / max(k, 0.000001);
    output[2u * n * n + index] = minus_i_h * mode.w / max(k, 0.000001);
    // exp(+i wt) travels along -k; exp(-i wt) travels along +k.
    // Continuity gives Q(k) = (omega/k) * k_hat * (b-a), not displacement.
    // Both elevation and momentum receive exactly the same spatial low-pass.
    let wavelength = 6.2831853 / max(k, 0.000001);
    let lowpass = smoothstep(params.min_wavelength, 2.0 * params.min_wavelength, wavelength);
    let flux = (b-a) * sqrt(9.81*k) / max(k*k,0.00000001) * lowpass;
    output[3u*n*n+index] = h * lowpass;
    output[4u*n*n+index] = flux * mode.z;
    output[5u*n*n+index] = flux * mode.w;
}

