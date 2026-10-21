// Local history in the undisplaced, periodic surface coordinates.
// Each invocation reads/writes only its own history cell: no ping-pong needed
// until adding spatial transport/diffusion.
struct Params {
    n: u32,
    length: f32,
    displacement_scale: f32,
    dt: f32,
    threshold: f32,
    generation: f32,
    decay: f32,
    reset: u32,
}
@group(0) @binding(0) var<storage, read> fields: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> history: array<vec2<f32>>;
@group(0) @binding(2) var<uniform> params: Params;

fn displacement(p: vec2<i32>) -> vec2<f32> {
    let n = i32(params.n);
    let wrapped = ((p % vec2(n)) + vec2(n)) % vec2(n);
    let index = u32(wrapped.y * n + wrapped.x);
    let count = params.n * params.n;
    return vec2(fields[count + index].x, fields[2u * count + index].x);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if index >= params.n * params.n { return; }
    let p = vec2<i32>(i32(index % params.n), i32(index / params.n));
    let factor = params.displacement_scale / (2.0 * params.length / f32(params.n));
    let ddx = (displacement(p + vec2(1, 0)) - displacement(p - vec2(1, 0))) * factor;
    let ddz = (displacement(p + vec2(0, 1)) - displacement(p - vec2(0, 1))) * factor;
    let j = (1.0 + ddx.x) * (1.0 + ddz.y) - ddz.x * ddx.y;

    var foam = history[index].x;
    if params.reset != 0u { foam = 0.0; }
    // Exact solution of dF/dt = source*(1-F) - decay*F for frozen source.
    // Per-second rates: pausing freezes history and changing FPS does not
    // change the lifetime of a foam patch.
    let source = params.generation * clamp(params.threshold - j, 0.0, 1.0);
    let rate = source + params.decay;
    if params.dt > 0.0 && rate > 0.0 {
        let equilibrium = source / rate;
        foam = equilibrium + (foam - equilibrium) * exp(-rate * params.dt);
    }
    history[index] = vec2(clamp(foam, 0.0, 1.0), j);
}
