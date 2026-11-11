// One optical path and material for ocean/SWE and bounded procedural water.
// Scene adapters provide the final surface position/normal and physical-path support.
struct WaterPath {
    background: vec3<f32>,
    distance: f32,
    ray: vec3<f32>,
    receiver: vec3<f32>,
    receiver_valid: bool,
    refracted_valid: bool,
}
fn water_path(input: WaterOut, normal: vec3<f32>, physical: bool) -> WaterPath {
    let incoming=normalize(input.world-camera.eye.xyz);
    let refracted=refract(incoming,normal,1.0/optics.refraction.y);
    var path=WaterPath(vec3(0.0),1000.0,incoming,vec3(0.0),false,false);
    let pixel=vec2<i32>(input.position.xy);
    let size=vec2<f32>(textureDimensions(scene_depth));
    let depth=textureLoad(scene_depth,pixel,0);
    if depth<1.0 {
        let uv=input.position.xy/size;
        let ndc=vec3(uv.x*2.0-1.0,1.0-uv.y*2.0,depth);
        let reconstructed=camera.inverse_view_projection*vec4(ndc,1.0);
        path.receiver=reconstructed.xyz/reconstructed.w;
        path.receiver_valid=true;
        path.distance=length(path.receiver-input.world);
        path.background=textureLoad(scene_color,pixel,0).rgb;
    }
    if optics.refraction.x>0.5 && camera.eye.y>input.world.y && dot(incoming,normal)<0.0 {
        if optics.refraction.x>1.5 {
            if path.receiver_valid {
                let hit=fast_refraction_hit(input.world,path.receiver,incoming,refracted,normal);
                if hit.valid {
                    let edge=min(min(hit.uv.x*size.x,hit.uv.y*size.y),min((1.0-hit.uv.x)*size.x,(1.0-hit.uv.y)*size.y));
                    let confidence=smoothstep(0.0,4.0,edge);
                    path.background=mix(path.background,fast_refraction_color(hit.uv,input.position.z,path.background),confidence);
                    path.distance=mix(path.distance,hit.distance,confidence);
                    path.ray=normalize(mix(incoming,normalize(hit.world-input.world),confidence));
                    path.receiver=hit.world;path.receiver_valid=true;path.refracted_valid=true;
                }
            }
        } else if physical {
            if shore.model.x>0.5 {
                let hit=trace_screen(input.world,refracted,optics.refraction.z,false);
                if hit.valid {
                    let uv=(vec2<f32>(hit.pixel)+vec2(0.5))/size;
                    let edge=min(min(uv.x*size.x,uv.y*size.y),min((1.0-uv.x)*size.x,(1.0-uv.y)*size.y));
                    let confidence=smoothstep(0.0,4.0,edge);
                    path.background=mix(path.background,textureLoad(scene_color,hit.pixel,0).rgb,confidence);
                    path.distance=mix(path.distance,hit.distance,confidence);
                    path.ray=normalize(mix(incoming,refracted,confidence));
                    path.receiver=screen_world(hit.pixel,textureLoad(scene_depth,hit.pixel,0));
                    path.receiver_valid=true;path.refracted_valid=true;
                }
            } else {
                let distance=water_bed_intersection(input.world,refracted);
                let hit=input.world+refracted*max(distance,0.0);
                let clip=camera.view_projection*vec4(hit,1.0);
                if distance>0.0 && distance<=optics.refraction.z && clip.w>0.0 {
                    let projected=clip.xyz/clip.w;
                    let uv=vec2(projected.x*0.5+0.5,0.5-projected.y*0.5);
                    if all(uv>=vec2(0.0)) && all(uv<vec2(1.0)) && projected.z>=0.0 && projected.z<=1.0 {
                        let hit_pixel=vec2<i32>(uv*size);
                        let hit_depth=textureLoad(scene_depth,hit_pixel,0);
                        let hit_uv=(vec2<f32>(hit_pixel)+vec2(0.5))/size;
                        let reconstructed=camera.inverse_view_projection*vec4(hit_uv.x*2.0-1.0,1.0-hit_uv.y*2.0,hit_depth,1.0);
                        let opaque=reconstructed.xyz/reconstructed.w;
                        let adjacent=camera.inverse_view_projection*vec4(hit_uv.x*2.0-1.0+2.0/size.x,1.0-hit_uv.y*2.0+2.0/size.y,hit_depth,1.0);
                        let footprint=length(adjacent.xyz/adjacent.w-opaque);
                        if hit_depth<1.0 && abs(opaque.y-water_bed_height(opaque.xz))<0.04
                            && length(opaque-hit)<max(0.04,footprint*1.5) {
                            let edge=min(min(uv.x*size.x,uv.y*size.y),min((1.0-uv.x)*size.x,(1.0-uv.y)*size.y));
                            let confidence=smoothstep(0.0,4.0,edge);
                            path.background=mix(path.background,textureLoad(scene_color,hit_pixel,0).rgb,confidence);
                            path.distance=mix(path.distance,distance,confidence);
                            path.ray=normalize(mix(incoming,refracted,confidence));
                            path.receiver=opaque;path.receiver_valid=true;path.refracted_valid=true;
                        }
                    }
                }
            }
        }
    }
    return path;
}
struct WaterLighting {
    color: vec3<f32>,
    medium: MediumResult,
    reflected: vec3<f32>,
    fresnel: f32,
}
fn shade_water(world: vec3<f32>, normal: vec3<f32>, path: WaterPath, caustic_debug: vec3<f32>) -> WaterLighting {
    let manual=integrate_medium(path.background,optics.sigma_depth.xyz,optics.source_view.xyz,path.distance);
    let skylight=integrate_skylight(world,path.ray,path.distance);
    let medium=MediumResult(manual.transmittance,manual.background,manual.scattering+skylight);
    let view=normalize(camera.eye.xyz-world);
    let incoming=-view;
    let fresnel=water_fresnel(max(dot(normal,view),0.0))*optics.options.y;
    let ssr=screen_reflection(world,normal,incoming);
    let reflected=mix(sky_with_halo(reflect(incoming,normal)),ssr.rgb,ssr.a);
    var color=mix(medium.background+medium.scattering,reflected,fresnel);
    color+=vec3(1.0,0.86,0.64)*0.7*water_sun_response(normal,view,camera.sun.xyz)*optics.options.y;
    let mode=u32(optics.source_view.w);
    if mode==1u { color=vec3(clamp(path.distance/optics.options.x,0.0,1.0)); }
    if mode==2u { color=medium.transmittance; }
    if mode==3u { color=medium.background; }
    if mode==4u { color=medium.scattering; }
    if mode==5u { color=path.background*(vec3(1.0)-medium.transmittance); }
    if mode==6u { color=path.background; }
    if mode==7u { color=medium.background+medium.scattering; }
    if mode==8u { color=sky_source_at(world+path.ray*path.distance*0.5); }
    if mode==9u { color=skylight; }
    if mode==10u { color=manual.scattering; }
    if mode==11u { color=optics.source_view.xyz+sky_source_at(world+path.ray*path.distance*0.5); }
    if mode==12u { color=caustic_debug; }
    if mode==13u { color=path.ray*0.5+vec3(0.5); }
    if mode==14u {
        color=select(vec3(0.8,0.05,0.02),vec3(0.02,0.8,0.1),path.refracted_valid);
        if optics.refraction.x<0.5 { color=vec3(0.2); }
    }
    if mode==15u { color=ssr.rgb*ssr.a; }
    return WaterLighting(color,medium,reflected,fresnel);
}
