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
    sand_mapping: vec4<f32>, // cos(rotation), sin(rotation), reserved
}
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> optics: Optics;
@group(0) @binding(2) var<storage, read> shore_field: array<vec4<f32>>;
@group(0) @binding(3) var<uniform> shore: ShoreParams;
@group(0) @binding(5) var sand_color: texture_2d<f32>;
@group(0) @binding(6) var sand_normal: texture_2d<f32>;
@group(0) @binding(7) var sand_roughness: texture_2d<f32>;
@group(0) @binding(8) var sand_ao: texture_2d<f32>;
@group(0) @binding(9) var sand_sampler: sampler;
@group(0) @binding(10) var<storage, read> terrain_bed: array<vec2<f32>>;
struct GroundOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) @interpolate(flat) local_grid: u32,
}
@vertex
fn ground_vertex(@builtin(vertex_index) index: u32) -> GroundOut {
    // Two real rasterized triangles, spanning the full 3x3 ocean footprint.
    let corners = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(-1.0, 1.0), vec2(1.0, -1.0),
                                    vec2(1.0, -1.0), vec2(-1.0, 1.0), vec2(1.0, 1.0));
    var xz=vec2(0.0); var height=0.0;
    if shore.model.x>0.5 && index>=6u {
        let n=u32(shore.options.z); let i=index-6u; let cell=i/6u;
        let uv=vec2<f32>(f32(cell%n),f32(cell/n))+(corners[i%6u]+vec2(1.0))*0.5;
        xz=shore.domain.xy+uv/f32(n)*shore.domain.zw;
        height=terrain_height(xz,shore);
    } else {
        xz=corners[index]*camera.ocean.y*1.5;
        height=select(terrain_height(xz,shore),shore.model.z,shore.model.x>0.5);
    }
    let world=vec3(xz.x,height,xz.y);
    return GroundOut(camera.view_projection*vec4(world,1.0),world,select(0u,1u,index>=6u));
}
@fragment
fn ground_fragment(input: GroundOut) -> @location(0) vec4<f32> {
    if shore.model.x>0.5 {
        let inside=all(input.world.xz>=shore.domain.xy) && all(input.world.xz<=shore.domain.xy+shore.domain.zw);
        if input.local_grid==0u && inside { discard; }
        // Actual scan triangles, not a second sand surface, occupy valid coverage.
        if input.local_grid==1u && model_terrain(input.world.xz,shore).y>0.5 { discard; }
    }
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
    var ambient_occlusion = 1.0;
    var roughness = 0.8;
    var sand_wetness = 0.0;
    if optics.beach.x > 0.5 {
        normal = normalize(vec3(optics.beach.y, 1.0, 0.0));
        // Tile size is in meters along the slope and along shore.
        // This is an artistic scale, not a measured source dimension.
        let slope_length = sqrt(1.0 + optics.beach.y * optics.beach.y);
        let plane = vec2(input.world.x * slope_length, input.world.z) / optics.beach.w;
        let c = optics.sand_mapping.x;
        let s = optics.sand_mapping.y;
        // Inverse rotation for sampling; rotate the normal basis forward below.
        let uv = vec2(c * plane.x + s * plane.y, -s * plane.x + c * plane.y);
        albedo = textureSample(sand_color, sand_sampler, uv).rgb;
        let tangent_normal = normalize(textureSample(sand_normal, sand_sampler, uv).xyz * 2.0 - 1.0);
        let tangent = normalize(vec3(1.0, -optics.beach.y, 0.0));
        // JPEG rows run downward; OpenGL normal +Y points toward image top.
        let bitangent = vec3(0.0, 0.0, -1.0);
        let rotated_tangent = c * tangent - s * bitangent;
        let rotated_bitangent = s * tangent + c * bitangent;
        normal = normalize(rotated_tangent * tangent_normal.x + rotated_bitangent * tangent_normal.y + normal * tangent_normal.z);
        roughness = clamp(textureSample(sand_roughness, sand_sampler, uv).r, 0.08, 1.0);
        ambient_occlusion = textureSample(sand_ao, sand_sampler, uv).r;
        var wetness = 1.0 - smoothstep(-0.15, 0.9, input.world.y);
        if shore_active(shore) {
            wetness = sample_shore_history(input.world.xz).y;
            // Submerged sand outside the narrow history strip remains wet.
            wetness = max(wetness, smoothstep(0.0, 0.15, -input.world.y));
            if shore.options.y > 3.5 && shore.options.y < 4.5 { return vec4(vec3(wetness), 1.0); }
        }
        albedo *= mix(1.0, select(0.70, 0.53, shore_active(shore)), wetness);
        sand_wetness = wetness;
        roughness = mix(roughness, max(0.12, roughness * 0.45), wetness);
    }
    if shore.model.x>0.5 {
        normal=normalize(cross(dpdy(input.world),dpdx(input.world)));
        if normal.y<0.0 { normal=-normal; }
    }
    let ndotl = max(dot(normal, camera.sun.xyz), 0.0);
    let lighting = 0.35 * ambient_occlusion + 0.65 * ndotl;
    var color = albedo * lighting;
    if optics.beach.x > 0.5 {
        let view = normalize(camera.eye.xyz - input.world);
        let half_vector = normalize(view + camera.sun.xyz);
        // Roughness-driven Blinn highlight, retaining the existing lightweight
        // lighting model rather than introducing a separate PBR renderer.
        let exponent = max(2.0, 2.0 / pow(roughness, 4.0) - 2.0);
        let strength = mix(0.035, 0.22, sand_wetness);
        color += vec3(strength) * ndotl * pow(max(dot(normal, half_vector), 0.0), exponent);
    }
    return vec4(color, 1.0);
}
