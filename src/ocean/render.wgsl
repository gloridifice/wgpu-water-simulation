@group(0) @binding(9) var<storage, read> shore_lace: array<f32>;
struct Camera {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    eye: vec4<f32>,
    sun: vec4<f32>,
    ocean: vec4<f32>,
    misc: vec4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> fields: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> foam_field: array<vec2<f32>>;
struct Optics {
    sigma_depth: vec4<f32>,
    source_view: vec4<f32>,
    options: vec4<f32>,
    sky_color: vec4<f32>,
    sky_albedo_enabled: vec4<f32>,
    sky_quality: vec4<f32>,
    beach: vec4<f32>,
}
@group(0) @binding(3) var<uniform> optics: Optics;
@group(0) @binding(4) var scene_color: texture_2d<f32>;
@group(0) @binding(5) var scene_depth: texture_depth_2d;
@group(0) @binding(6) var sky_lut: texture_2d<f32>;
@group(0) @binding(7) var<storage, read> shore_field: array<vec4<f32>>;
@group(0) @binding(8) var<uniform> shore: ShoreParams;

fn sky_color(direction: vec3<f32>) -> vec3<f32> {
    var color = sky_background(direction, optics.sky_color);
    let sun = max(dot(direction, camera.sun.xyz), 0.0);
    color += vec3(1.0, 0.75, 0.40) * (0.20 * pow(sun, 40.0) + 3.0 * pow(sun, 3000.0));
    return color;
}

struct SkyOut { @builtin(position) position: vec4<f32>, @location(0) ndc: vec2<f32> }
@vertex
fn sky_vertex(@builtin(vertex_index) index: u32) -> SkyOut {
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    return SkyOut(vec4(x, y, 1.0, 1.0), vec2(x, y));
}
@fragment
fn sky_fragment(input: SkyOut) -> @location(0) vec4<f32> {
    let world = camera.inverse_view_projection * vec4(input.ndc, 1.0, 1.0);
    return vec4(sky_color(normalize(world.xyz / world.w - camera.eye.xyz)), 1.0);
}

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

struct WaterOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) grid: vec2<f32>,
}
@vertex
fn water_vertex(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> WaterOut {
    let n = u32(camera.misc.y);
    let grid = vec2<f32>(f32(index % (n + 1u)), f32(index / (n + 1u))) * camera.ocean.x / f32(n);
    let tile = vec2<i32>(i32(instance % 3u) - 1, i32(instance / 3u) - 1);
    let offset = vec2<f32>(tile) * camera.ocean.y;
    let p = surface(grid, offset);
    let delta = select(1.0, min(1.0, camera.ocean.x / f32(n)), shore_active(shore));
    let tx = surface(grid + vec2(delta, 0), offset) - surface(grid - vec2(delta, 0), offset);
    let tz = surface(grid + vec2(0, delta), offset) - surface(grid - vec2(0, delta), offset);
    return WaterOut(camera.view_projection * vec4(p, 1.0), p, normalize(cross(tz, tx)), grid);
}
fn foam_at(grid: vec2<i32>) -> vec2<f32> {
    let n = i32(camera.ocean.x);
    let wrapped = ((grid % vec2(n)) + vec2(n)) % vec2(n);
    return foam_field[u32(wrapped.y * n + wrapped.x)];
}
fn sample_foam(grid: vec2<f32>) -> vec2<f32> {
    let cell = vec2<i32>(floor(grid));
    let f = fract(grid);
    return mix(mix(foam_at(cell), foam_at(cell + vec2(1, 0)), f.x),
               mix(foam_at(cell + vec2(0, 1)), foam_at(cell + vec2(1, 1)), f.x), f.y);
}
fn foam_hash(cell: vec2<f32>) -> f32 {
    // Integer number of detail repeats per tile: no seam across periodic blocks.
    let period = camera.ocean.x * 4.0;
    let wrapped = cell - floor(cell / period) * period;
    return fract(sin(dot(wrapped, vec2(127.1, 311.7))) * 43758.5453);
}
fn foam_detail(grid: vec2<f32>) -> f32 {
    let p = grid * 4.0;
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (vec2(3.0) - 2.0 * f);
    let noise = mix(mix(foam_hash(cell), foam_hash(cell + vec2(1, 0)), u.x),
                    mix(foam_hash(cell + vec2(0, 1)), foam_hash(cell + vec2(1, 1)), u.x), u.y);
    // Fade subpixel detail to its mean to avoid sparkling at distance.
    return mix(noise, 0.5, smoothstep(0.5, 2.0, length(fwidth(p))));
}
@fragment
fn water_fragment(input: WaterOut) -> @location(0) vec4<f32> {
    let shore_history = sample_shore_history(input.world.xz);
    let local_depth = max(input.world.y - bottom_height(input.world.xz, shore), 0.0);
    if shore_active(shore) {
        if local_depth < 0.008 { discard; }
        if shore.options.y > 0.5 && shore.options.y < 1.5 {
            return vec4(vec3(clamp(local_depth / 3.0, 0.0, 1.0)), 1.0);
        }
        if shore.options.y > 1.5 && shore.options.y < 2.5 {
            let u = shore_history.w;
            return vec4(mix(vec3(0.08), select(vec3(0.95, 0.32, 0.08), vec3(0.05, 0.65, 1.0), u < 0.0), clamp(abs(u) / 2.0, 0.0, 1.0)), 1.0);
        }
        if shore.options.y > 2.5 && shore.options.y < 3.5 { return vec4(vec3(shore_history.x), 1.0); }
        if shore.options.y > 4.5 { return vec4(vec3(sample_shore_extra(input.world.xz).x), 1.0); }
        if shore.options.y > 3.5 { return vec4(vec3(shore_history.y), 1.0); }
    }
    let pixel = vec2<i32>(input.position.xy);
    let size = vec2<f32>(textureDimensions(scene_depth));
    let depth = textureLoad(scene_depth, pixel, 0);
    // A missing opaque hit is treated as an optically deep, finite water ray.
    // Avoid infinities so zero extinction and a nonzero source remain defined.
    var distance = 1000.0;
    var bg = vec3(0.0);
    if depth < 1.0 {
        let uv = input.position.xy / size;
        let ndc = vec3(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth);
        let reconstructed = camera.inverse_view_projection * vec4(ndc, 1.0);
        let bottom = reconstructed.xyz / reconstructed.w;
        distance = length(bottom - input.world);
        bg = textureLoad(scene_color, pixel, 0).rgb;
    }
    let ray = normalize(input.world - camera.eye.xyz);
    let manual = integrate_medium(bg, optics.sigma_depth.xyz, optics.source_view.xyz, distance);
    let skylight = integrate_skylight(input.world, ray, distance);
    let medium = MediumResult(manual.transmittance, manual.background, manual.scattering + skylight);
    let mode = u32(optics.source_view.w);
    if mode == 1u { return vec4(vec3(clamp(distance / optics.options.x, 0.0, 1.0)), 1.0); }
    if mode == 2u { return vec4(medium.transmittance, 1.0); }
    if mode == 3u { return vec4(medium.background, 1.0); }
    if mode == 4u { return vec4(medium.scattering, 1.0); }
    if mode == 5u { return vec4(bg * (vec3(1.0) - medium.transmittance), 1.0); }
    if mode == 6u { return vec4(bg, 1.0); }
    if mode == 7u { return vec4(medium.background + medium.scattering, 1.0); }
    if mode == 8u { return vec4(sky_source_at(input.world + ray * distance * 0.5), 1.0); }
    if mode == 9u { return vec4(skylight, 1.0); }
    if mode == 10u { return vec4(manual.scattering, 1.0); }
    if mode == 11u { return vec4(optics.source_view.xyz + sky_source_at(input.world + ray * distance * 0.5), 1.0); }
    var foam_data = sample_foam(input.grid);
    foam_data.x = 1.0 - (1.0 - foam_data.x * ocean_weight(input.world.xz, shore)) * (1.0 - shore_history.x);
    if camera.misc.z > 1.5 { return vec4(vec3(foam_data.x), 1.0); }
    if camera.misc.z > 0.5 {
        let j = foam_data.y;
        if j < 0.0 { return vec4(0.9, 0.04, 0.02, 1.0); }
        if j < 1.0 { return vec4(mix(vec3(1.0, 0.25, 0.02), vec3(1.0), j), 1.0); }
        return vec4(mix(vec3(1.0), vec3(0.02, 0.3, 0.9), clamp(j - 1.0, 0.0, 1.0)), 1.0);
    }
    let normal = normalize(input.normal);
    let view = normalize(camera.eye.xyz - input.world);
    let facing = max(dot(normal, view), 0.0);
    let fresnel = (0.02 + 0.98 * pow(1.0 - facing, 5.0)) * optics.options.y;
    let reflected = sky_color(reflect(-view, normal));
    var color = mix(medium.background + medium.scattering, reflected, fresnel);
    let half_vector = normalize(view + camera.sun.xyz);
    let specular = pow(max(dot(normal, half_vector), 0.0), 300.0);
    color += vec3(1.0, 0.86, 0.64) * specular * 1.8 * optics.options.y;
    // Foam is matte and scatters light; mix after water highlights/reflections.
    // World coordinates prevent repeated shoreline patterns on the 3x3 tiles.
    var detail = foam_detail(input.grid);
    var foam = camera.misc.x * smoothstep(0.015, 0.25, foam_data.x) * (0.55 + 0.45 * smoothstep(0.15, 0.75, detail));
    if shore_active(shore) {
        let flow=vec2(shore_history.w,sample_shore_extra(input.world.xz).w);
        let lace=0.72*flowing_lace(input.world.xz,flow,12.0)+0.28*flowing_lace(input.world.xz,flow,1.2);
        let amount=pow(shore_history.x,1.5)*smoothstep(0.003,0.02,local_depth);
        let aa=max(0.04,fwidth(lace));
        let shore_foam=smoothstep(1.0-amount-aa,1.0-amount+0.38+aa,lace)*smoothstep(0.0,0.02,amount);
        let deep_foam = smoothstep(0.015, 0.25, sample_foam(input.grid).x)
            * ocean_weight(input.world.xz, shore) * (0.55 + 0.45 * foam_detail(input.grid));
        foam = camera.misc.x * (1.0 - (1.0 - deep_foam) * (1.0 - shore_foam));
    }
    let foam_color = vec3(0.87, 0.92, 0.90) * (0.65 + 0.35 * max(dot(normal, camera.sun.xyz), 0.0));
    color = mix(color, foam_color, foam);
    return vec4(color, 1.0);
}
