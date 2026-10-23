@group(0) @binding(4) var<storage, read> shore_lace: array<f32>;
struct Camera {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    sun: vec4<f32>,
    ocean: vec4<f32>,
    misc: vec4<f32>,
}
struct Optics {
    sigma_depth: vec4<f32>,
    source_view: vec4<f32>,
    options: vec4<f32>,
    sky_color: vec4<f32>,
    sky_albedo_enabled: vec4<f32>,
    sky_quality: vec4<f32>,
    beach: vec4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> optics: Optics;
@group(0) @binding(2) var<storage, read> shore_field: array<vec4<f32>>;
@group(0) @binding(3) var<uniform> shore: ShoreParams;
struct GroundOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
}
@vertex
fn ground_vertex(@builtin(vertex_index) index: u32) -> GroundOut {
    // Two real rasterized triangles, spanning the full 3x3 ocean footprint.
    let corners = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(-1.0, 1.0), vec2(1.0, -1.0),
                                    vec2(1.0, -1.0), vec2(-1.0, 1.0), vec2(1.0, 1.0));
    let xz = corners[index] * camera.ocean.y * 1.5;
    let height = terrain_height(xz, shore);
    let world = vec3(xz.x, height, xz.y);
    return GroundOut(camera.view_projection * vec4(world, 1.0), world);
}
@fragment
fn ground_fragment(input: GroundOut) -> @location(0) vec4<f32> {
    let p = input.world.xz / optics.options.w;
    // Integrate a checker analytically over a pixel footprint to limit aliasing.
    let w = max(fwidth(p), vec2(0.001));
    let a = abs(fract((p - 0.5 * w) * 0.5) - 0.5);
    let b = abs(fract((p + 0.5 * w) * 0.5) - 0.5);
    let filtered = 2.0 * (a - b) / w;
    let checker = clamp(0.5 - 0.5 * filtered.x * filtered.y, 0.0, 1.0);
    var albedo = mix(vec3(0.20, 0.16, 0.09), vec3(0.72, 0.64, 0.43), checker);
    // Broad RGB bands make wavelength-dependent loss easy to inspect.
    if optics.options.z > 0.5 {
        let band = u32(floor((input.world.x + camera.ocean.y * 1.5) / 8.0)) % 4u;
        var tint = vec3(1.0);
        if band == 0u { tint = vec3(1.0, 0.14, 0.14); }
        if band == 1u { tint = vec3(0.14, 1.0, 0.14); }
        if band == 2u { tint = vec3(0.14, 0.14, 1.0); }
        albedo = mix(vec3(0.18), vec3(0.85), checker) * tint;
    }
    var normal = vec3(0.0, 1.0, 0.0);
    if optics.beach.x > 0.5 {
        normal = normalize(vec3(optics.beach.y, 1.0, 0.0));
        // Sand material is independent of water color; the gradient is produced
        // by the real sloped depth buffer and the existing medium integration.
        let variation = 0.5 + 0.25 * sin(input.world.x * 0.31 + sin(input.world.z * 0.16))
            + 0.25 * sin(input.world.z * 0.47 + input.world.x * 0.13);
        let ripple_phase = input.world.x * 7.0 + 0.8 * sin(input.world.z * 0.7);
        let ripple = sin(ripple_phase) * (1.0 - smoothstep(0.5, 2.0, fwidth(ripple_phase)));
        albedo = mix(vec3(0.56, 0.39, 0.19), vec3(0.86, 0.72, 0.46), variation);
        albedo *= 1.0 + 0.035 * ripple;
        var wetness = 1.0 - smoothstep(-0.15, 0.9, input.world.y);
        if shore_active(shore) {
            wetness = sample_shore_history(input.world.xz).y;
            // Submerged sand outside the narrow history strip remains wet.
            wetness = max(wetness, smoothstep(0.0, 0.15, -input.world.y));
            if shore.options.y > 4.5 && shore.options.y < 5.5 { return vec4(vec3(foam_density_level(sample_shore_extra(input.world.xz).x,shore)), 1.0); }
            if shore.options.y > 3.5 && shore.options.y < 4.5 { return vec4(vec3(wetness), 1.0); }
        }
        albedo *= mix(1.0, select(0.70, 0.53, shore_active(shore)), wetness);
    }
    if shore_active(shore) && shore.options.y < 5.5 {
        let stranded=sample_shore_extra(input.world.xz).x;
        let lace=lace_sample(input.world.xz/9.0);
        let aa=max(0.03,fwidth(lace));
        // Concentration controls opacity, not a threshold against the lace.
        // A threshold erased dilute deposits (especially after mip filtering).
        let pattern=mix(0.35,1.0,smoothstep(0.2-aa,0.75+aa,lace));
        let mask=pow(foam_density_level(stranded,shore),0.65)*pattern*smoothstep(0.0,0.02,stranded);
        albedo=mix(albedo,shore.stranded_foam_color.rgb,mask*camera.misc.x);
    }
    let lighting = 0.35 + 0.65 * max(dot(normal, camera.sun.xyz), 0.0);
    var color = albedo * lighting;
    if shore_active(shore) {
        let wetness = sample_shore_history(input.world.xz).y;
        let view = normalize(camera.eye.xyz - input.world);
        let half_vector = normalize(view + camera.sun.xyz);
        color += vec3(0.20, 0.22, 0.23) * wetness * pow(max(dot(normal, half_vector), 0.0), 100.0);
    }
    if shore_active(shore) && shore.options.y > 5.5 {
        let stranded=sample_shore_extra(input.world.xz).x;
        color=mix(color,foam_density_color(stranded,shore),smoothstep(0.0,0.01,stranded));
    }
    return vec4(color, 1.0);
}
