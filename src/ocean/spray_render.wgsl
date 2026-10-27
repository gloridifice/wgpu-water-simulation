struct Camera {
    view_projection: mat4x4<f32>, inverse_view_projection: mat4x4<f32>,
    eye:vec4<f32>, sun:vec4<f32>, ocean:vec4<f32>, misc:vec4<f32>,
}
@group(0) @binding(0) var<uniform> spray:SprayParams;
@group(0) @binding(1) var<uniform> camera:Camera;
@group(0) @binding(2) var<storage,read> particles:ParticleList;
@group(0) @binding(3) var scene_depth:texture_depth_2d;
@group(0) @binding(4) var sprite:texture_2d_array<f32>;
@group(0) @binding(5) var sprite_sampler:sampler;
@group(0) @binding(6) var<storage,read> signals:array<SpraySignal>;
struct VertexOut {
    @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32>,
    @location(1) world:vec3<f32>, @location(2) color:vec4<f32>,
    @location(3) @interpolate(flat) layer:u32,
}
@vertex
fn vertex(@builtin(vertex_index) index:u32,@builtin(instance_index) instance:u32) -> VertexOut {
    let corners=array<vec2<f32>,6>(vec2(-1,-1),vec2(1,-1),vec2(-1,1),vec2(-1,1),vec2(1,-1),vec2(1,1));
    let corner=corners[index];
    var p:Particle;
    var color=vec4(0.9,0.95,1.0,1.0);
    if spray.style.w>0.5 {
        let s=signals[instance];
        if s.position_amount.w<=0.0 { return VertexOut(vec4(2.0,2.0,2.0,1.0),vec2(0.0),vec3(0.0),vec4(0.0),0u); }
        p.position_age=vec4(s.position_amount.xyz/s.position_amount.w+vec3(0.0,0.1,0.0),0.0);
        p.appearance=vec4(sqrt(spray.grid.x)*0.8,0.0,0.0,0.0);p.velocity_life=vec4(0.0,0.0,0.0,1.0);
        let rate=s.position_amount.w/max(spray.grid.y*spray.grid.x,0.000001);
        let level=clamp(rate/max(spray.emission.x,0.000001),0.0,1.0);
        color=vec4(mix(vec3(0.0,1.0,0.1),vec3(1.0,0.1,0.0),level),0.9);
    } else {
        p=particles.items[instance];
        let age=p.position_age.w/p.velocity_life.w;
        color.w=smoothstep(0.0,0.08,age)*(1.0-smoothstep(0.55,1.0,age))*0.8;
        color=vec4(color.xyz*(0.55+0.45*max(camera.sun.y,0.0)),color.w);
    }
    let direction=normalize(camera.eye.xyz-p.position_age.xyz);
    let right=normalize(cross(vec3(0.0,1.0,0.0),direction));
    let up=cross(direction,right);
    let cs=cos(p.appearance.y);let sn=sin(p.appearance.y);
    // Rotate geometry, not UVs: rotating UVs would cut the splash against the quad.
    let rotated=vec2(cs*corner.x-sn*corner.y,sn*corner.x+cs*corner.y);
    let world=p.position_age.xyz+(right*rotated.x+up*rotated.y)*p.appearance.x*0.5;
    return VertexOut(camera.view_projection*vec4(world,1.0),corner*0.5+vec2(0.5),world,color,min(u32(p.appearance.z),3u));
}
@fragment
fn fragment(input:VertexOut) -> @location(0) vec4<f32> {
    let texel=vec2<i32>(input.position.xy);
    let depth=textureLoad(scene_depth,texel,0);
    if input.position.z>depth { discard; }
    let size=vec2<f32>(textureDimensions(scene_depth));
    let ndc=vec2(input.position.x/size.x*2.0-1.0,1.0-input.position.y/size.y*2.0);
    let background=camera.inverse_view_projection*vec4(ndc,depth,1.0);
    let distance=length(background.xyz/background.w-camera.eye.xyz)-length(input.world-camera.eye.xyz);
    let soft=clamp(distance/spray.style.y,0.0,1.0);
    let texture_mask=textureSample(sprite,sprite_sampler,input.uv,i32(input.layer)).r;
    // Keep the source-strength diagnostic independent of the chosen art asset.
    let debug_mask=1.0-smoothstep(0.3,0.5,length(input.uv-vec2(0.5)));
    let mask=select(texture_mask,debug_mask,spray.style.w>0.5);
    let alpha=mask*input.color.w*soft;
    return vec4(input.color.xyz*alpha,alpha);
}
