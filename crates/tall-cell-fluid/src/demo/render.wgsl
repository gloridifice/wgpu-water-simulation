// 不透明预览材质：实际模拟三角形 + 光照和 Fresnel 天空反射。
struct Uniforms {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    camera: vec4<f32>,
    extent: vec4<f32>,
    control: vec4<u32>,
}
@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct SurfaceOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) barycentric: vec3<f32>,
}

fn safe_normalize(value: vec3<f32>) -> vec3<f32> {
    let magnitude = dot(value, value);
    if magnitude < 1e-20 { return vec3<f32>(0.0, 1.0, 0.0); }
    return value * inverseSqrt(magnitude);
}

fn sky(direction: vec3<f32>) -> vec3<f32> {
    let up = clamp(direction.y * 0.5 + 0.5, 0.0, 1.0);
    let horizon = vec3<f32>(0.43, 0.58, 0.65);
    let zenith = vec3<f32>(0.08, 0.19, 0.29);
    let ground = vec3<f32>(0.035, 0.055, 0.070);
    let color = mix(ground, mix(horizon, zenith, pow(up, 1.8)), smoothstep(0.35, 0.53, up));
    let sun = pow(max(dot(direction, normalize(vec3<f32>(-0.4, 0.8, 0.3))), 0.0), 180.0);
    return color + vec3<f32>(1.0, 0.87, 0.66) * sun * 2.0;
}

@vertex
fn water_vertex(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>,
    @location(2) barycentric: vec3<f32>) -> SurfaceOutput {
    var output: SurfaceOutput;
    output.clip = uniforms.view_projection * vec4<f32>(position, 1.0);
    output.world = position;
    output.normal = normal;
    output.barycentric = barycentric;
    return output;
}

@fragment
fn water_fragment(input: SurfaceOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // 导数在分支之前执行，网格覆盖不依赖 polygon-line 设备 feature。
    let width = max(fwidth(input.barycentric), vec3<f32>(1e-6));
    let edge = smoothstep(vec3<f32>(0.0), width * 1.2, input.barycentric);
    let wire = 1.0 - min(edge.x, min(edge.y, edge.z));
    let outward = safe_normalize(input.normal);
    var normal = outward;
    if !front { normal = -normal; }
    let view = safe_normalize(uniforms.camera.xyz - input.world);
    let facing = max(dot(normal, view), 0.0);
    let fresnel = 0.02 + 0.98 * pow(1.0-facing, 5.0);
    let light = normalize(vec3<f32>(-0.4, 0.8, 0.3));
    let diffuse = 0.38 + 0.62 * max(dot(normal, light), 0.0);
    let height = clamp(input.world.y / max(uniforms.extent.y, 1e-6), 0.0, 1.0);
    let body = mix(vec3<f32>(0.012, 0.12, 0.17), vec3<f32>(0.02, 0.33, 0.36), height) * diffuse;
    let reflected = sky(reflect(-view, normal));
    let half_vector = safe_normalize(view + light);
    let highlight = pow(max(dot(normal, half_vector), 0.0), 120.0) * 0.8;
    var color = mix(body, reflected, fresnel * 0.85 + 0.08) + vec3<f32>(0.78, 0.9, 0.95) * highlight;
    if uniforms.control.x == 1u {
        color = mix(body * 0.65 + vec3<f32>(0.02), vec3<f32>(0.12, 0.72, 0.74), wire * 0.88);
    }
    if uniforms.control.x == 2u { color = outward * 0.5 + vec3<f32>(0.5); }
    return vec4<f32>(color, 1.0);
}

struct GroundOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) screen: vec2<f32>,
}

@vertex
fn ground_vertex(@builtin(vertex_index) vertex: u32) -> GroundOutput {
    var output: GroundOutput;
    let point = vec2<f32>(f32((vertex << 1u) & 2u), f32(vertex & 2u));
    output.screen = point * 2.0 - vec2<f32>(1.0);
    output.clip = vec4<f32>(output.screen, 1.0, 1.0);
    return output;
}

struct GroundFragment {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
fn ground_fragment(input: GroundOutput) -> GroundFragment {
    let far_h = uniforms.inverse_view_projection * vec4<f32>(input.screen, 1.0, 1.0);
    let ray = safe_normalize(far_h.xyz / far_h.w - uniforms.camera.xyz);
    let denominator = select(max(ray.y, 1e-6), min(ray.y, -1e-6), ray.y < 0.0);
    let ground_height = -0.007 * uniforms.extent.w;
    let distance = (ground_height - uniforms.camera.y) / denominator;
    let position = uniforms.camera.xyz + ray * clamp(distance, 0.0, uniforms.extent.w * 200.0);
    let spacing = max(uniforms.extent.w / 12.0, 1e-5);
    let grid = position.xz / spacing;
    let derivative = max(fwidth(grid), vec2<f32>(1e-6));
    let edge_distance = abs(fract(grid - vec2<f32>(0.5)) - vec2<f32>(0.5)) / derivative;
    let line = 1.0 - min(min(edge_distance.x, edge_distance.y), 1.0);
    let inside = position.x >= 0.0 && position.x <= uniforms.extent.x
        && position.z >= 0.0 && position.z <= uniforms.extent.z;
    let base = select(vec3<f32>(0.018, 0.026, 0.038), vec3<f32>(0.035, 0.075, 0.09), inside);
    let line_color = select(vec3<f32>(0.065, 0.095, 0.12), vec3<f32>(0.075, 0.15, 0.17), inside);
    let fade = exp(-max(distance, 0.0) / (uniforms.extent.w * 12.0));
    let floor_color = mix(base, line_color, line * fade * 0.65);
    let background = vec3<f32>(0.015, 0.023, 0.036)
        + vec3<f32>(0.014, 0.023, 0.032) * (1.0-clamp(input.screen.y*0.5+0.5, 0.0, 1.0));
    let projected = uniforms.view_projection * vec4<f32>(position, 1.0);
    let floor_visible = distance > 0.0 && ray.y < 0.0 && projected.w > 0.0;
    var result: GroundFragment;
    result.color = vec4<f32>(select(background, mix(background, floor_color, fade), floor_visible), 1.0);
    result.depth = select(1.0, clamp(projected.z / projected.w, 0.0, 1.0), floor_visible);
    return result;
}

@vertex
fn frame_vertex(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return uniforms.view_projection * vec4<f32>(position, 1.0);
}

@fragment
fn frame_fragment() -> @location(0) vec4<f32> {
    return vec4<f32>(0.13, 0.25, 0.30, 1.0);
}
