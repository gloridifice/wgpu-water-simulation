struct SprayParams {
    emission: vec4<f32>, // rate /m²/s, slope, rise m/s, depth m
    motion: vec4<f32>,   // upward m/s, forward m/s, lifetime s, diameter m
    style: vec4<f32>,    // drag /s, soft distance m, simulated dt, debug
    counts: vec4<u32>,   // capacity, source cells, seed, reserved
    grid: vec4<f32>,     // source cell area m², last source dt for paused debug, reserved
}
struct SpraySignal { position_amount: vec4<f32>, velocity_reserved: vec4<f32> }
struct Particle { position_age: vec4<f32>, velocity_life: vec4<f32>, appearance: vec4<f32> }
struct ParticleList { count: u32, padding: array<u32,3>, items: array<Particle> }
struct ParticleOutput { count: atomic<u32>, padding: array<u32,3>, items: array<Particle> }
