#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShoreUniform {
    pub terrain: [f32; 4],
    pub waves: [f32; 4],
    pub domain: [f32; 4],
    pub timing: [f32; 4],
    pub coupling: [f32; 4],
    pub filter: [f32; 4],
    pub solver: [f32; 4],
    pub visual: [f32; 4],
    pub density: [f32; 4],
    pub foam_generation: [f32; 4],
    pub foam_front: [f32; 4],
    pub foam_ridge: [f32; 4],
    pub foam_velocity: [f32; 4],
    pub foam_motion: [f32; 4],
    pub foam_inertia: [f32; 4],
    pub water_foam_color: [f32; 4],
    pub foam_material: [f32; 4],
    pub foam_shading: [f32; 4],
    pub foam_lifetime: [f32; 4], // dual mode, fresh/residual exit rates /s, conversion fraction
    pub foam_fresh_threshold: [f32; 4], // mode, constant, main/detail frequencies /m; render only
    pub foam_fresh_noise: [f32; 4], // detail weight, reserved; render only
    pub foam_fresh_material: [f32; 4], // N-only NSH source, size m, micro normal, roughness
    pub foam_fresh_shading: [f32; 4], // N-only pore coverage reference, reserved
    pub foam_fresh_opacity: [f32; 4], // N concentration opacity: enabled, range, power, reserved
    pub foam_residual_opacity: [f32; 4], // R concentration opacity: enabled, range, power, reserved
    pub options: [f32; 4],
    pub collider_meta: [f32; 4],
    pub model: [f32; 4], // enabled, minimum bed, fallback bed, sponge width
    pub edges: [f32; 4], // open -X,+X,-Z,+Z
    pub colliders: [crate::ocean::scene::collider::ColliderUniform;
        crate::ocean::scene::collider::MAX_COLLIDERS],
}
