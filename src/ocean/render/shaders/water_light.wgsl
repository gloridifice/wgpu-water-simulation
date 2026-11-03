// Shared plane geometry and air -> water light transport, in world meters.
fn water_bed_normal() -> vec3<f32> {
    return normalize(vec3(select(0.0, optics.beach.y, optics.beach.x > 0.5 && shore.model.x<0.5), 1.0, 0.0));
}
fn water_bed_height(xz: vec2<f32>) -> f32 {
    if shore.model.x>0.5 { return model_terrain(xz,shore).x; }
    return select(-optics.sigma_depth.w, -optics.beach.y * (xz.x - optics.beach.z), optics.beach.x > 0.5);
}
fn water_bed_intersection(origin: vec3<f32>, direction: vec3<f32>) -> f32 {
    let slope = select(0.0, optics.beach.y, optics.beach.x > 0.5 && shore.model.x<0.5);
    let denominator = direction.y + slope * direction.x;
    if denominator >= -0.00001 { return -1.0; }
    let bed=select(water_bed_height(origin.xz),shore.model.z,shore.model.x>0.5);
    return (bed - origin.y) / denominator;
}
fn water_fresnel(cosine: f32) -> f32 {
    if optics.refraction.y<=1.000001 { return 0.0; }
    let r = (optics.refraction.y - 1.0) / (optics.refraction.y + 1.0);
    return r*r + (1.0-r*r)*pow(1.0-clamp(cosine,0.0,1.0),5.0);
}
fn caustics_center() -> vec2<f32> {
    return select(vec2(0.0), shore.domain.xy + 0.5 * shore.domain.zw, shore_active(shore) || shore.model.x>0.5);
}
fn caustics_uv(world: vec2<f32>) -> vec2<f32> {
    return (world-caustics_center()) / optics.caustics.z + vec2(0.5);
}
fn flat_water_light(world: vec3<f32>, surface_height: f32) -> vec3<f32> {
    let sun = normalize(camera.sun.xyz);
    if sun.y <= 0.0 { return vec3(0.0); }
    let ray = refract(-sun, vec3(0.0,1.0,0.0), 1.0 / optics.refraction.y);
    let path = max(surface_height-world.y,0.0) / max(-ray.y,0.001);
    return vec3(sun.y / max(-ray.y,0.001) * max(dot(water_bed_normal(),-ray),0.0) * (1.0-water_fresnel(sun.y)))
        * exp(-optics.sigma_depth.xyz*path);
}
