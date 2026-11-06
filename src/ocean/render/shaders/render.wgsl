@group(0) @binding(14) var caustic_map: texture_2d<f32>;
@group(0) @binding(15) var caustic_sampler: sampler;
@group(0) @binding(16) var scene_ranges: texture_2d<f32>;
@group(0) @binding(9) var<storage, read> shore_lace: array<f32>;
@group(0) @binding(10) var foam_nsh: texture_2d_array<f32>;
@group(0) @binding(11) var foam_nsh_sampler: sampler;
@group(0) @binding(13) var<storage, read> terrain_bed: array<vec2<f32>>;
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> fields: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> foam_field: array<vec2<f32>>;
@group(0) @binding(3) var<uniform> optics: Optics;
@group(0) @binding(4) var scene_color: texture_2d<f32>;
@group(0) @binding(5) var scene_depth: texture_depth_2d;
@group(0) @binding(6) var sky_lut: texture_2d<f32>;
@group(0) @binding(7) var<storage, read> shore_field: array<vec4<f32>>;
@group(0) @binding(8) var<uniform> shore: ShoreParams;

fn sky_with_halo(direction: vec3<f32>) -> vec3<f32> {
    let sun = max(dot(direction, camera.sun.xyz), 0.0);
    return sky_background(direction, optics.sky_color)
        + vec3(1.0, 0.75, 0.40) * (0.20 * pow(sun, 40.0));
}

fn sky_color(direction: vec3<f32>) -> vec3<f32> {
    let sun = max(dot(direction, camera.sun.xyz), 0.0);
    return sky_with_halo(direction) + vec3(1.0, 0.75, 0.40) * (3.0 * pow(sun, 3000.0));
}

// Cook–Torrance dielectric reflection under a directional sun: BRDF times N·L.
fn water_sun_response(normal: vec3<f32>, view: vec3<f32>, light: vec3<f32>) -> f32 {
    let no_v = clamp(dot(normal, view), 0.0, 1.0);
    let no_l = clamp(dot(normal, light), 0.0, 1.0);
    if no_v <= 0.0 || no_l <= 0.0 || light.y <= 0.0 {
        return 0.0;
    }
    let half_sum = view + light;
    let half_length_squared = dot(half_sum, half_sum);
    if half_length_squared < 1e-8 {
        return 0.0;
    }
    let half_vector = half_sum * inverseSqrt(half_length_squared);
    let no_h = clamp(dot(normal, half_vector), 0.0, 1.0);
    let lo_h = clamp(dot(light, half_vector), 0.0, 1.0);

    // Residual roughness below the scale of the resolved wave normal.
    let roughness = 0.28;
    let alpha = roughness * roughness;
    let alpha_squared = alpha * alpha;
    let no_h_squared = no_h * no_h;
    let ggx = (1.0 - no_h_squared) + alpha_squared * no_h_squared;
    let distribution = alpha_squared / (3.14159265 * ggx * ggx);
    // Height-correlated Smith visibility (includes the BRDF denominator).
    let shadow_view = no_l * sqrt(no_v * no_v * (1.0 - alpha_squared) + alpha_squared);
    let shadow_light = no_v * sqrt(no_l * no_l * (1.0 - alpha_squared) + alpha_squared);
    let visibility_times_cosine = 0.5 * no_l / (shadow_view + shadow_light);
    return distribution * visibility_times_cosine * water_fresnel(lo_h);
}

struct SkyOut { @builtin(position) position: vec4<f32>, @location(0) ndc: vec2<f32> }
// RGB is the sampled scene radiance; alpha is SSR's contribution weight.
fn screen_reflection(origin:vec3<f32>, normal:vec3<f32>, incoming:vec3<f32>) -> vec4<f32> {
    if optics.reflection.x<0.5 || incoming.y>=0.0 || dot(normal,-incoming)<=0.0 {
        return vec4(0.0);
    }
    let maximum=optics.reflection.y;
    let hit=trace_screen(origin,reflect(incoming,normal),maximum,true);
    if !hit.valid { return vec4(0.0); }
    let size=vec2<f32>(textureDimensions(scene_depth));
    let pixel=vec2<f32>(hit.pixel)+vec2(0.5);
    let edge=min(min(pixel.x,pixel.y),min(size.x-pixel.x,size.y-pixel.y));
    let edge_weight=smoothstep(0.0,max(optics.reflection.w,0.0001),edge);
    let confidence=optics.reflection.z*edge_weight*(1.0-smoothstep(0.8*maximum,maximum,hit.distance));
    return vec4(textureLoad(scene_color,hit.pixel,0).rgb,confidence);
}
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
fn water_vertex_data(uv: vec2<f32>, instance: u32) -> WaterOut {
    let grid = uv * camera.ocean.x;
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
fn water_vertex(@location(0) uv: vec2<f32>, @builtin(instance_index) instance: u32) -> WaterOut {
    return water_vertex_data(uv, instance);
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
// Additional optical factor, separate from pore coverage and simulated density.
// A zero range is the limiting step: positive concentration is fully normalized.
fn concentration_opacity(density: f32, settings: vec4<f32>) -> f32 {
    if settings.x<0.5 { return 1.0; }
    if settings.y<=0.0 { return select(0.0,1.0,density>0.0); }
    return pow(clamp(density/settings.y,0.0,1.0),settings.z);
}
fn material_grid(material: vec2<f32>) -> f32 {
    let p=material*2.0;
    let d=abs(fract(p-0.5)-0.5);
    let line=vec2(1.0)-smoothstep(vec2(0.0),max(fwidth(p),vec2(0.001)),d);
    return max(line.x,line.y);
}
// The same NSH renderer serves legacy foam and N, with independent settings.
// settings: source (1..3), tile size m, micro normal, optical density scale.
fn nsh_material_phase(material: vec2<f32>, world: vec3<f32>, base: vec3<f32>, view: vec3<f32>, density: f32, depth: f32, settings: vec4<f32>, coverage_reference: f32, roughness: f32) -> FoamAppearance {
    let layer=i32(settings.x)-1;
    let uv=material/settings.y;
    let coarse=textureSample(foam_nsh,foam_nsh_sampler,uv,layer);
    let fine=textureSample(foam_nsh,foam_nsh_sampler,uv*10.0,layer);
    let pattern=0.72*coarse.ba+0.28*fine.ba;
    let opacity=nsh_opacity(density,depth,pattern.x,max(0.04,fwidth(pattern.x)),coverage_reference,settings.w);
    let wx=dpdx(world.xz); let wy=dpdy(world.xz);
    let mx=dpdx(material); let my=dpdy(material);
    let determinant=wx.x*wy.y-wx.y*wy.x;
    var jacobian=vec4(1.0,0.0,0.0,1.0);
    if abs(determinant)>0.00000001 {
        jacobian=vec4((mx*wy.y-my*wx.y)/determinant,(my*wx.x-mx*wy.x)/determinant);
    }
    let slope=0.72*nsh_slope(coarse)+0.28*nsh_slope(fine);
    let n=nsh_normal(base,slope,jacobian,settings.z);
    let light=max(dot(n,camera.sun.xyz),0.0);
    let half_vector=normalize(view+camera.sun.xyz);
    let exponent=max(2.0,2.0/pow(roughness,4.0)-2.0);
    let highlight=pow(max(dot(n,half_vector),0.0),exponent)*light*0.04;
    let color=shore.water_foam_color.rgb*(0.65+0.35*light)+vec3(highlight);
    return FoamAppearance(color*opacity,opacity,pattern,n);
}
fn foam_material_phase(material: vec2<f32>, world: vec3<f32>, base: vec3<f32>, view: vec3<f32>, density: f32, species: vec2<f32>, depth: f32) -> FoamAppearance {
    if shore.foam_lifetime.x>0.5 {
        let old=0.55*prototype_noise(material*2.5)+0.30*prototype_noise(material*7.0)+0.15*prototype_noise(material*17.0);
        let fresh_amount=select(species.x,0.0,shore.density.w>1.5);
        let residual_amount=select(species.y,0.0,shore.density.w>0.5 && shore.density.w<1.5);
        let fresh_factor=concentration_opacity(fresh_amount,shore.foam_fresh_opacity);
        let residue=residual_opacity(residual_amount,depth,old,fwidth(old),shore.foam_material.w)
            *concentration_opacity(residual_amount,shore.foam_residual_opacity);
        let color=shore.water_foam_color.rgb*(0.65+0.35*max(dot(base,camera.sun.xyz),0.0));
        if shore.foam_fresh_threshold.x>1.5 {
            let fresh=nsh_material_phase(material,world,base,view,fresh_amount,depth,
                vec4(shore.foam_fresh_material.xyz,shore.foam_material.w),
                shore.foam_fresh_shading.x,shore.foam_fresh_material.w);
            // Scale both premultiplied color and alpha, not just RGB, so the
            // factor reveals the layer underneath instead of darkening the foam.
            let white=fresh.opacity*fresh_factor;
            // N is over R. Its micro normal/lighting never shades the residual.
            return FoamAppearance(fresh.premultiplied*fresh_factor+color*(0.72*residue*(1.0-white)),
                white+residue*(1.0-white),vec2(fresh.pattern.x,old),
                select(base,fresh.normal,white>0.0));
        }
        // Preserve the constant/noise density response and soft transitions.
        var fresh=shore.foam_fresh_threshold.y;
        if shore.foam_fresh_threshold.x>0.5 {
            // Decorrelate the two octave orientations as well as their frequencies.
            let fine=vec2(0.8*material.x-0.6*material.y,0.6*material.x+0.8*material.y);
            let weight=shore.foam_fresh_noise.x;
            fresh=(1.0-weight)*filtered_fresh_noise(material*shore.foam_fresh_threshold.z)
                +weight*filtered_fresh_noise(fine*shore.foam_fresh_threshold.w+vec2(17.3,-9.1));
        }
        let white=prototype_opacity(fresh_amount,depth,fresh,0.5*fwidth(fresh),shore.foam_material.w)*fresh_factor;
        return FoamAppearance(color*(white+0.72*residue*(1.0-white)),
            white+residue*(1.0-white),vec2(fresh,old),base);
    }
    if shore.foam_shading.x>0.5 {
        let grain=0.55*prototype_noise(material*2.5)+0.30*prototype_noise(material*7.0)+0.15*prototype_noise(material*17.0);
        let opacity=prototype_opacity(density,depth,grain,0.5*fwidth(grain),shore.foam_material.w);
        let color=shore.water_foam_color.rgb*(0.65+0.35*max(dot(base,camera.sun.xyz),0.0));
        return FoamAppearance(color*opacity,opacity,vec2(grain,0.0),base);
    }
    return nsh_material_phase(material,world,base,view,density,depth,
        shore.foam_material,shore.density.y,shore.foam_shading.y);
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
    // Water and lifted foam share the detail slope, not their base normals.
    let detail_slope = shore_detail_slope(input.grid, input.world, local_depth, detail_footprint);
    let normal = apply_detail_slope(normalize(input.normal), detail_slope);
    let incoming=normalize(input.world-camera.eye.xyz);
    let refracted=refract(incoming,normal,1.0/optics.refraction.y);
    var refracted_valid=false;
    var water_ray=incoming;
    let pixel = vec2<i32>(input.position.xy);
    let size = vec2<f32>(textureDimensions(scene_depth));
    let depth = textureLoad(scene_depth, pixel, 0);
    // A missing opaque hit is treated as an optically deep, finite water ray.
    // Avoid infinities so zero extinction and a nonzero source remain defined.
    var distance = 1000.0;
    var bg = vec3(0.0);
    var receiver=vec3(0.0);
    var receiver_valid=false;
    if depth < 1.0 {
        let uv = input.position.xy / size;
        let ndc = vec3(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth);
        let reconstructed = camera.inverse_view_projection * vec4(ndc, 1.0);
        let bottom = reconstructed.xyz / reconstructed.w;
        receiver=bottom; receiver_valid=true;
        distance = length(bottom - input.world);
        bg = textureLoad(scene_color, pixel, 0).rgb;
    }
    if optics.refraction.x>0.5 && camera.eye.y>input.world.y && dot(incoming,normal)<0.0 {
        if optics.refraction.x>1.5 {
            if receiver_valid {
                let hit=fast_refraction_hit(input.world,receiver,incoming,refracted,normal);
                if hit.valid {
                    let edge=min(min(hit.uv.x*size.x,hit.uv.y*size.y),min((1.0-hit.uv.x)*size.x,(1.0-hit.uv.y)*size.y));
                    let confidence=smoothstep(0.0,4.0,edge);
                    bg=mix(bg,fast_refraction_color(hit.uv,input.position.z,bg),confidence);
                    distance=mix(distance,hit.distance,confidence);
                    water_ray=normalize(mix(incoming,normalize(hit.world-input.world),confidence));
                    receiver=hit.world; receiver_valid=true; refracted_valid=true;
                }
            }
        } else if shore.model.x>0.5 {
            let hit=trace_screen(input.world,refracted,optics.refraction.z,false);
            if hit.valid {
                let uv=(vec2<f32>(hit.pixel)+vec2(0.5))/size;
                let edge=min(min(uv.x*size.x,uv.y*size.y),min((1.0-uv.x)*size.x,(1.0-uv.y)*size.y));
                let confidence=smoothstep(0.0,4.0,edge);
                bg=mix(bg,textureLoad(scene_color,hit.pixel,0).rgb,confidence);
                distance=mix(distance,hit.distance,confidence);
                water_ray=normalize(mix(incoming,refracted,confidence));
                receiver=screen_world(hit.pixel,textureLoad(scene_depth,hit.pixel,0));
                receiver_valid=true; refracted_valid=true;
            }
        } else {
            let path=water_bed_intersection(input.world,refracted);
            let hit=input.world+refracted*max(path,0.0);
            let clip=camera.view_projection*vec4(hit,1.0);
            if path>0.0 && path<=optics.refraction.z && clip.w>0.0 {
                let projected=clip.xyz/clip.w;
                let uv=vec2(projected.x*0.5+0.5,0.5-projected.y*0.5);
                if all(uv>=vec2(0.0)) && all(uv<vec2(1.0)) && projected.z>=0.0 && projected.z<=1.0 {
                    let hit_pixel=vec2<i32>(uv*size);
                    let hit_depth=textureLoad(scene_depth,hit_pixel,0);
                    let hit_uv=(vec2<f32>(hit_pixel)+vec2(0.5))/size;
                    let reconstructed=camera.inverse_view_projection*vec4(hit_uv.x*2.0-1.0,1.0-hit_uv.y*2.0,hit_depth,1.0);
                    let opaque=reconstructed.xyz/reconstructed.w;
                    let adjacent=camera.inverse_view_projection*vec4(hit_uv.x*2.0-1.0+2.0/size.x,1.0-hit_uv.y*2.0+2.0/size.y,hit_depth,1.0);
                    let pixel_footprint=length(adjacent.xyz/adjacent.w-opaque);
                    // A visible foreground object must never be stretched onto the
                    // analytic seabed. Missing/offscreen hits retain the original ray.
                    if hit_depth<1.0 && abs(opaque.y-water_bed_height(opaque.xz))<0.04
                        && length(opaque-hit)<max(0.04,pixel_footprint*1.5) {
                        let edge=min(min(uv.x*size.x,uv.y*size.y),min((1.0-uv.x)*size.x,(1.0-uv.y)*size.y));
                        let confidence=smoothstep(0.0,4.0,edge);
                        bg=mix(bg,textureLoad(scene_color,hit_pixel,0).rgb,confidence);
                        distance=mix(distance,path,confidence);
                        water_ray=normalize(mix(incoming,refracted,confidence));
                        receiver=opaque; receiver_valid=true;
                        refracted_valid=true;
                    }
                }
            }
        }
    }
    var model_light=vec3(0.0);
    if shore.model.x>0.5 && optics.caustics.x>0.5 && receiver_valid {
        let grid=shore.domain.zw/shore.options.z;
        let bed=model_terrain(receiver.xz,shore).x;
        let receiver_mask=1.0-smoothstep(max(0.04,min(grid.x,grid.y)*0.3),max(0.08,min(grid.x,grid.y)*0.8),abs(receiver.y-bed));
        let wave=surface((receiver.xz/camera.ocean.y+vec2(0.5))*camera.ocean.x,vec2(0.0)).y;
        if receiver.y<wave-0.015 {
            model_light=caustic_light(receiver,wave)*receiver_mask;
            // Photogrammetry already contains baked lighting. Add the focused
            // excess instead of multiplying a second directional light into it.
            bg*=vec3(1.0)+0.65*optics.caustics.y*max(model_light-flat_water_light(receiver,wave),vec3(0.0));
        }
    }
    let ray = water_ray;
    let manual = integrate_medium(bg, optics.sigma_depth.xyz, optics.source_view.xyz, distance);
    let skylight = integrate_skylight(input.world, ray, distance);
    let medium = MediumResult(manual.transmittance, manual.background, manual.scattering + skylight);
    let mode = u32(optics.source_view.w);
    if mode == 15u {
        let ssr=screen_reflection(input.world,normal,incoming);
        return vec4(ssr.rgb*ssr.a,1.0);
    }
    if mode == 12u {
        if shore.model.x>0.5 { return vec4(model_light,1.0); }
        return vec4(select(vec3(0.0),bg,optics.caustics.x>0.5),1.0);
    }
    if mode == 13u { return vec4(water_ray*0.5+vec3(0.5),1.0); }
    if mode == 14u {
        if optics.refraction.x<0.5 { return vec4(0.2,0.2,0.2,1.0); }
        return vec4(select(vec3(0.8,0.05,0.02),vec3(0.02,0.8,0.1),refracted_valid),1.0);
    }
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

    let view = normalize(camera.eye.xyz - input.world);
    let facing = max(dot(normal, view), 0.0);
    let fresnel = water_fresnel(facing) * optics.options.y;
    let reflection_ray = reflect(-view, normal);
    let ssr=screen_reflection(input.world,normal,incoming);
    // The sky's visible sun disc is represented here by direct BRDF lighting.
    let reflected=mix(sky_with_halo(reflection_ray),ssr.rgb,ssr.a);
    var color = mix(medium.background + medium.scattering, reflected, fresnel);
    // Scene-relative irradiance for the current SDR output.
    let sun_irradiance = vec3(1.0, 0.86, 0.64) * 0.7;
    color += sun_irradiance * water_sun_response(normal, view, camera.sun.xyz) * optics.options.y;
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
    let foam_normal = apply_detail_slope(normalize(input.foam_normal), detail_slope);
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
