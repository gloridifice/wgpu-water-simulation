@group(0) @binding(9) var<storage, read> shore_lace: array<f32>;
@group(0) @binding(10) var foam_nsh: texture_2d_array<f32>;
@group(0) @binding(11) var foam_nsh_sampler: sampler;
@group(0) @binding(13) var<storage, read> terrain_bed: array<vec2<f32>>;
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
    sand_mapping: vec4<f32>, // cos(rotation), sin(rotation), reserved
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
    @location(3) foam_normal: vec3<f32>,
    @location(4) @interpolate(flat) local_grid: u32,
    @location(5) base: vec2<f32>,
}
// Visual foam thickness never enters surface(), SWE state, wet/dry tests or
// water optics. Sample the same world-space density used by foam shading.
fn foam_vertex_surface(water: vec3<f32>) -> vec3<f32> {
    if !shore_active(shore) || shore.visual.y <= 0.0 || camera.misc.x <= 0.0 { return water; }
    let history = sample_shore_history(water.xz);
    var amount = max(history.x, 0.0);
    if shore.foam_lifetime.x>0.5 && shore.density.w>0.5 {
        let species=sample_shore_species(water.xz);
        amount=select(species.x,species.y,shore.density.w>1.5);
    }
    let weight = smoothstep(shore.visual.z, shore.visual.z + shore.visual.w, amount);
    let wet = smoothstep(0.008, 0.02, history.z);
    return water + vec3(0.0, shore.visual.y * weight * wet, 0.0);
}
fn water_vertex_data(index: u32, instance: u32) -> WaterOut {
    let n = u32(camera.misc.y);
    let grid = vec2<f32>(f32(index % (n + 1u)), f32(index / (n + 1u))) * camera.ocean.x / f32(n);
    let tile = vec2<i32>(i32(instance % 3u) - 1, i32(instance / 3u) - 1);
    let offset = vec2<f32>(tile) * camera.ocean.y;
    return water_from_grid(grid,offset,0u);
}
fn water_from_grid(grid: vec2<f32>, offset: vec2<f32>, local_grid: u32) -> WaterOut {
    let n=u32(camera.misc.y);
    let p = surface(grid, offset);
    var delta = select(1.0, min(1.0, camera.ocean.x / f32(n)), shore_active(shore));
    if local_grid==1u {
        delta=min(1.0,camera.ocean.x/camera.ocean.y*min(shore.domain.z,shore.domain.w)/shore.options.z);
    }
    let xp = surface(grid + vec2(delta, 0), offset);
    let xm = surface(grid - vec2(delta, 0), offset);
    let zp = surface(grid + vec2(0, delta), offset);
    let zm = surface(grid - vec2(0, delta), offset);
    let foam_tx = foam_vertex_surface(xp) - foam_vertex_surface(xm);
    let foam_tz = foam_vertex_surface(zp) - foam_vertex_surface(zm);
    return WaterOut(camera.view_projection * vec4(foam_vertex_surface(p), 1.0),
        p, normalize(cross(zp - zm, xp - xm)), grid, normalize(cross(foam_tz, foam_tx)),local_grid,
        (grid/camera.ocean.x-vec2(0.5))*camera.ocean.y+offset);
}
@vertex
fn water_vertex(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> WaterOut {
    return water_vertex_data(index, instance);
}
@vertex
fn local_water_vertex(@builtin(vertex_index) index: u32) -> WaterOut {
    let n=u32(shore.options.z);
    let uv=vec2<f32>(f32(index%(n+1u)),f32(index/(n+1u)))/f32(n);
    let world=shore.domain.xy+uv*shore.domain.zw;
    return water_from_grid((world/camera.ocean.y+vec2(0.5))*camera.ocean.x,vec2(0.0),1u);
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
// Render-only high-frequency slope: subtract a wider finite difference from
// the local FFT slope. This is independent of the SWE inlet filter and does
// not add displacement or momentum. Derivatives suppress subpixel detail.
fn fft_detail_slope(grid: vec2<f32>, step: f32) -> vec2<f32> {
    let dx = sample_field(grid + vec2(step, 0.0), 0u) - sample_field(grid - vec2(step, 0.0), 0u);
    let dz = sample_field(grid + vec2(0.0, step), 0u) - sample_field(grid - vec2(0.0, step), 0u);
    return vec2(dx, dz) * camera.ocean.w / (2.0 * step * camera.ocean.y / camera.ocean.x);
}
fn shore_detail_normal(base: vec3<f32>, grid: vec2<f32>, world: vec3<f32>, depth: f32, footprint: f32) -> vec3<f32> {
    if !shore_active(shore) || shore.visual.x <= 0.0 { return base; }
    let weight = (1.0 - ocean_weight(world.xz, shore)) * smoothstep(0.02, 0.25, depth)
        * (1.0 - smoothstep(0.5, 2.0, footprint)) * shore.visual.x;
    if weight <= 0.0 { return base; }
    let slope = (fft_detail_slope(grid, 1.0) - fft_detail_slope(grid, 4.0)) * weight;
    return normalize(vec3(base.x - base.y * slope.x, base.y, base.z - base.y * slope.y));
}

// Keep the existing density-dependent pores, but use unclipped density for
// optical opacity. Changing opacity_scale never changes the pore threshold.
// This is a render-only response, not a physical foam thickness or mass change.
fn nsh_opacity(density: f32, depth: f32, soft: f32, soft_width: f32, coverage_reference: f32, opacity_scale: f32) -> f32 {
    let amount=max(density,0.0);
    let m=pow(clamp(amount/coverage_reference,0.0,1.0),1.5)*smoothstep(0.003,0.02,depth);
    let sw=max(soft_width,0.001);
    let threshold=mix(1.0+sw,-sw,m);
    let coverage=smoothstep(threshold-sw,threshold+sw,soft)*smoothstep(0.0,0.02,m);
    let optical_depth=amount/opacity_scale;
    // Avoid cancellation for tiny densities without a discontinuous cutoff.
    var opacity=1.0-exp(-optical_depth);
    if optical_depth<0.001 { opacity=optical_depth*(1.0-0.5*optical_depth); }
    return coverage*opacity;
}

// Apply the material-coordinate Jacobian to the decoded normal slopes. This
// keeps highlights aligned with foam patterns when their advected UVs shear.
fn nsh_normal(base: vec3<f32>, slope: vec2<f32>, jacobian: vec4<f32>, strength: f32) -> vec3<f32> {
    var offset=vec3(dot(slope,jacobian.xy),0.0,dot(slope,jacobian.zw))*strength;
    offset-=base*dot(offset,base);
    offset*=min(1.0,8.0/max(length(offset),0.0001));
    return normalize(base+offset);
}
fn nsh_slope(data: vec4<f32>) -> vec2<f32> {
    let xy=data.rg*2.0-vec2(1.0);
    return xy/max(sqrt(max(0.0,1.0-dot(xy,xy))),0.25);
}
struct FoamAppearance {
    premultiplied: vec3<f32>,
    opacity: f32,
    pattern: vec2<f32>,
    normal: vec3<f32>,
}
// Prototype pores live in advected world coordinates, not screen space.
fn prototype_noise(p: vec2<f32>) -> f32 {
    let footprint=max(length(dpdx(p)),length(dpdy(p)));
    return mix(shore_noise(p),0.5,smoothstep(0.5,2.0,footprint));
}
// Fresh white patches must not reveal the square value-noise lattice when
// sparse. Use compact gradient kernels on a skewed triangular lattice instead;
// unlike interpolated random corner values, extrema are not pinned to grid nodes.
fn fresh_gradient(cell: vec2<i32>) -> vec2<f32> {
    let q=bitcast<vec2<u32>>(cell);
    var hash=q.x*1597334677u ^ q.y*3812015801u;
    hash=(hash ^ (hash>>16u))*2246822519u;
    hash=hash ^ (hash>>13u);
    let angle=f32(hash & 65535u)*(6.28318530718/65536.0);
    return vec2(cos(angle),sin(angle));
}
fn fresh_kernel(cell: vec2<i32>, offset: vec2<f32>) -> f32 {
    let support=max(0.5-dot(offset,offset),0.0);
    let squared=support*support;
    return squared*squared*dot(fresh_gradient(cell),offset);
}
fn fresh_noise(p: vec2<f32>) -> f32 {
    let skew=0.36602540378; let unskew=0.21132486541;
    let cell=floor(p+vec2((p.x+p.y)*skew));
    let local=p-cell+vec2((cell.x+cell.y)*unskew);
    let side=select(vec2(0,1),vec2(1,0),local.x>local.y);
    let lattice=vec2<i32>(cell);
    let value=fresh_kernel(lattice,local)
        +fresh_kernel(lattice+side,local-vec2<f32>(side)+vec2(unskew))
        +fresh_kernel(lattice+vec2(1),local-vec2(1.0)+vec2(2.0*unskew));
    return clamp(0.5+35.0*value,0.0,1.0);
}
fn filtered_fresh_noise(p: vec2<f32>) -> f32 {
    let footprint=max(length(dpdx(p)),length(dpdy(p)));
    return mix(fresh_noise(p),0.5,smoothstep(0.5,2.0,footprint));
}
fn prototype_opacity(density: f32, depth: f32, grain: f32, width: f32, scale: f32) -> f32 {
    let coverage=1.0-exp(-max(density,0.0)/scale);
    // The original threshold can leak white at zero density for low grain.
    return smoothstep(grain-0.11-width,grain+0.08+width,coverage)
        *smoothstep(0.0,0.02,coverage)*smoothstep(0.003,0.02,depth);
}
fn residual_opacity(density: f32, depth: f32, grain: f32, width: f32, scale: f32) -> f32 {
    let coverage=1.0-exp(-max(0.0,density)/scale);
    // Narrow contour bands become filaments/spots as R falls; only appearance,
    // not resolved turbulence. Derivative filtering suppresses subpixel flicker.
    let band=abs(grain-0.5)*2.0;
    let aa=0.025+width;
    return (1.0-smoothstep(coverage-aa,coverage+aa,band))
        *smoothstep(0.0,0.02,coverage)*smoothstep(0.003,0.02,depth);
}
fn material_grid(material: vec2<f32>) -> f32 {
    let p=material*2.0;
    let d=abs(fract(p-0.5)-0.5);
    let line=vec2(1.0)-smoothstep(vec2(0.0),max(fwidth(p),vec2(0.001)),d);
    return max(line.x,line.y);
}
fn foam_material_phase(material: vec2<f32>, world: vec3<f32>, base: vec3<f32>, view: vec3<f32>, density: f32, species: vec2<f32>, depth: f32) -> FoamAppearance {
    if shore.foam_lifetime.x>0.5 {
        // Decorrelate the two octave orientations as well as their frequencies.
        let fine=vec2(0.8*material.x-0.6*material.y,0.6*material.x+0.8*material.y);
        let fresh=0.7*filtered_fresh_noise(material*0.8)+0.3*filtered_fresh_noise(fine*2.5+vec2(17.3,-9.1));
        let old=0.55*prototype_noise(material*2.5)+0.30*prototype_noise(material*7.0)+0.15*prototype_noise(material*17.0);
        let fresh_amount=select(species.x,0.0,shore.density.w>1.5);
        let residual_amount=select(species.y,0.0,shore.density.w>0.5 && shore.density.w<1.5);
        let white=prototype_opacity(fresh_amount,depth,fresh,0.5*fwidth(fresh),shore.foam_material.w);
        let residue=residual_opacity(residual_amount,depth,old,fwidth(old),shore.foam_material.w);
        let color=shore.water_foam_color.rgb*(0.65+0.35*max(dot(base,camera.sun.xyz),0.0));
        return FoamAppearance(color*(white+0.72*residue*(1.0-white)),
            white+residue*(1.0-white),vec2(fresh,old),base);
    }
    if shore.foam_shading.x>0.5 {
        let grain=0.55*prototype_noise(material*2.5)+0.30*prototype_noise(material*7.0)+0.15*prototype_noise(material*17.0);
        let opacity=prototype_opacity(density,depth,grain,0.5*fwidth(grain),shore.foam_material.w);
        let color=shore.water_foam_color.rgb*(0.65+0.35*max(dot(base,camera.sun.xyz),0.0));
        return FoamAppearance(color*opacity,opacity,vec2(grain,0.0),base);
    }
    let layer=i32(shore.foam_material.x)-1;
    let uv=material/shore.foam_material.y;
    let coarse=textureSample(foam_nsh,foam_nsh_sampler,uv,layer);
    let fine=textureSample(foam_nsh,foam_nsh_sampler,uv*10.0,layer);
    let pattern=0.72*coarse.ba+0.28*fine.ba;
    let opacity=nsh_opacity(density,depth,pattern.x,max(0.04,fwidth(pattern.x)),shore.density.y,shore.foam_material.w);
    let wx=dpdx(world.xz); let wy=dpdy(world.xz);
    let mx=dpdx(material); let my=dpdy(material);
    let determinant=wx.x*wy.y-wx.y*wy.x;
    var jacobian=vec4(1.0,0.0,0.0,1.0);
    if abs(determinant)>0.00000001 {
        jacobian=vec4((mx*wy.y-my*wx.y)/determinant,(my*wx.x-mx*wy.x)/determinant);
    }
    let slope=0.72*nsh_slope(coarse)+0.28*nsh_slope(fine);
    let n=nsh_normal(base,slope,jacobian,shore.foam_material.z);
    let light=max(dot(n,camera.sun.xyz),0.0);
    let half_vector=normalize(view+camera.sun.xyz);
    let roughness=shore.foam_shading.y;
    let exponent=max(2.0,2.0/pow(roughness,4.0)-2.0);
    let highlight=pow(max(dot(n,half_vector),0.0),exponent)*light*0.04;
    let color=shore.water_foam_color.rgb*(0.65+0.35*light)+vec3(highlight);
    return FoamAppearance(color*opacity,opacity,pattern,n);
}

@fragment
fn water_fragment(input: WaterOut) -> @location(0) vec4<f32> {
    if shore.model.x>0.5 && shore_active(shore) && input.local_grid==0u {
        if all(input.base>=shore.domain.xy) && all(input.base<=shore.domain.xy+shore.domain.zw) { discard; }
    }
    let detail_footprint = max(length(dpdx(input.grid)), length(dpdy(input.grid)));
    let shore_history = sample_shore_history(input.world.xz);
    let geometric_depth = max(input.world.y - bottom_height(input.world.xz, shore), 0.0);
    if shore.model.x>0.5 && geometric_depth<0.008 { discard; }
    if shore_active(shore) && shore.collider_meta.x>0.5 {
        if bottom_height(input.world.xz,shore)>terrain_height(input.world.xz,shore)+0.00001 && geometric_depth<0.008 { discard; }
    }
    // The render mesh is coarser than SWE: interpolated geometry can bridge dry
    // cells and cover exposed sand. Use simulated depth for nearshore coverage.
    var local_depth=geometric_depth;
    if shore_active(shore) {
        local_depth=mix(max(shore_history.z,0.0),geometric_depth,ocean_weight(input.world.xz,shore));
    }
    if shore_active(shore) {
        if local_depth < 0.008 { discard; }
        if shore.options.y > 0.5 && shore.options.y < 1.5 {
            return vec4(vec3(clamp(local_depth / 3.0, 0.0, 1.0)), 1.0);
        }
        if shore.options.y > 1.5 && shore.options.y < 2.5 {
            let u = shore_history.w;
            return vec4(mix(vec3(0.08), select(vec3(0.95, 0.32, 0.08), vec3(0.05, 0.65, 1.0), u < 0.0), clamp(abs(u) / 2.0, 0.0, 1.0)), 1.0);
        }
        if shore.options.y > 6.5 && shore.options.y < 8.5 {
            let diagnostic_values=sample_shore_extra(input.world.xz).xy;
            var value=diagnostic_values.y;
            if shore.options.y<7.5 { value=diagnostic_values.x/max(shore.foam_front.x,0.000001); }
            return vec4(vec3(clamp(value,0.0,1.0)),1.0);
        }
        if shore.options.y > 2.5 && shore.options.y < 3.5 { return vec4(vec3(foam_density_level(shore_history.x,shore)), 1.0); }
        if shore.options.y > 3.5 && shore.options.y < 4.5 { return vec4(vec3(shore_history.y), 1.0); }
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
    foam_data.x = 1.0 - (1.0 - foam_data.x * ocean_weight(input.world.xz, shore)) * (1.0 - foam_density_level(shore_history.x,shore));
    if camera.misc.z > 1.5 { return vec4(vec3(foam_data.x), 1.0); }
    if camera.misc.z > 0.5 {
        let j = foam_data.y;
        if j < 0.0 { return vec4(0.9, 0.04, 0.02, 1.0); }
        if j < 1.0 { return vec4(mix(vec3(1.0, 0.25, 0.02), vec3(1.0), j), 1.0); }
        return vec4(mix(vec3(1.0), vec3(0.02, 0.3, 0.9), clamp(j - 1.0, 0.0, 1.0)), 1.0);
    }
    let normal = shore_detail_normal(normalize(input.normal), input.grid, input.world, local_depth, detail_footprint);
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
    if shore_active(shore) && shore.foam_material.x<0.5 && shore.foam_shading.x<0.5 && shore.foam_lifetime.x<0.5 {
        let texture_motion=sample_shore_texture(input.world.xz);
        let material_a=input.world.xz-texture_motion.offsets.xy;
        let material_b=input.world.xz-texture_motion.offsets.zw;
        let lace_a=0.72*lace_sample(material_a/12.0)+0.28*lace_sample(material_a/1.2);
        let lace_b=0.72*lace_sample(material_b/12.0)+0.28*lace_sample(material_b/1.2);
        let amount=pow(foam_density_level(shore_history.x,shore),1.5)*smoothstep(0.003,0.02,local_depth);
        let aa_a=max(0.04,fwidth(lace_a)); let aa_b=max(0.04,fwidth(lace_b));
        let coverage_a=smoothstep(1.0-amount-aa_a,1.0-amount+0.38+aa_a,lace_a);
        let coverage_b=smoothstep(1.0-amount-aa_b,1.0-amount+0.38+aa_b,lace_b);
        let shore_foam=mix(coverage_b,coverage_a,texture_motion.blend)*smoothstep(0.0,0.02,amount);
        let deep_foam = smoothstep(0.015, 0.25, sample_foam(input.grid).x)
            * ocean_weight(input.world.xz, shore) * (0.55 + 0.45 * foam_detail(input.grid));
        foam = camera.misc.x * (1.0 - (1.0 - deep_foam) * (1.0 - shore_foam));
    }
    let foam_normal = shore_detail_normal(normalize(input.foam_normal), input.grid, input.world, local_depth, detail_footprint);
    let foam_color = shore.water_foam_color.rgb * (0.65 + 0.35 * max(dot(foam_normal, camera.sun.xyz), 0.0));
    if shore_active(shore) && (shore.foam_material.x>0.5 || shore.foam_shading.x>0.5 || shore.foam_lifetime.x>0.5) {
        // Each phase is thresholded and shaded before cross-fading. Blending
        // raw Soft/Height first would open/close unrelated holes at handoff.
        let motion=sample_shore_texture(input.world.xz);
        let species=sample_shore_species(input.world.xz);
        let a=foam_material_phase(input.world.xz-motion.offsets.xy,input.world,foam_normal,view,shore_history.x,species,local_depth);
        let b=foam_material_phase(input.world.xz-motion.offsets.zw,input.world,foam_normal,view,shore_history.x,species,local_depth);
        let opacity=mix(b.opacity,a.opacity,motion.blend)*camera.misc.x;
        let contribution=mix(b.premultiplied,a.premultiplied,motion.blend)*camera.misc.x;
        // Offshore history retains its existing material.
        let show_offshore=select(1.0,0.0,shore.foam_lifetime.x>0.5 && shore.density.w>0.5);
        let deep=show_offshore*smoothstep(0.015,0.25,sample_foam(input.grid).x)*ocean_weight(input.world.xz,shore)
            *(0.55+0.45*foam_detail(input.grid))*camera.misc.x;
        color=mix(color,foam_color,deep)*(1.0-opacity)+contribution;
        if shore.options.y<0.5 && camera.misc.x>0.5 {
            let debug=u32(shore.foam_shading.z);
            let pattern=mix(b.pattern,a.pattern,motion.blend);
            if debug==1u { return vec4(vec3(pattern.x),1.0); }
            if debug==2u { return vec4(vec3(pattern.y),1.0); }
            if debug==3u { return vec4(normalize(mix(b.normal,a.normal,motion.blend))*0.5+vec3(0.5),1.0); }
            if debug==4u { return vec4(vec3(opacity),1.0); }
            if debug==5u {
                let grid=mix(material_grid(input.world.xz-motion.offsets.zw),material_grid(input.world.xz-motion.offsets.xy),motion.blend);
                return vec4(vec3(grid)*smoothstep(0.003,0.02,local_depth),1.0);
            }
        }
    } else {
        color = mix(color, foam_color, foam);
    }
    if shore_active(shore) && shore.options.y > 5.5 {
        // No texture, lighting or FFT foam in density debug colors.
        var amount=shore_history.x;
        if shore.options.y>8.5 {
            let species=sample_shore_species(input.world.xz);
            amount=select(species.x,species.y,shore.options.y>9.5);
        }
        let presence=smoothstep(0.0,0.01,amount);
        color=mix(mix(medium.background+medium.scattering,reflected,fresnel),
            foam_density_color(amount,shore),presence);
    }
    return vec4(color, 1.0);
}
