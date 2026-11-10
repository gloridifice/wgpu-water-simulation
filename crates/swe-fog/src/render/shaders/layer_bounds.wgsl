// Nonnegative reconstruction weights make this support envelope conservative.
// Keep the leaf support identical to the fragment sampler, including masks,
// invalid cells, boundary clamping and the selected raw/filtered h/m source.
struct LayerBounds { lower:vec4f, upper:vec4f }
fn compute_layer_bounds(anchor:vec2i,dims:vec2i,reconstruction:f32)->LayerBounds {
    var bounds=LayerBounds(vec4f(1e20),vec4f(-1e20));
    if any(anchor>=dims)||bed[anchor.y*dims.x+anchor.x].y<=0.5{return bounds;}
    let cubic=reconstruction==1.0;
    let radius=select(1,2,cubic);
    for(var z=-radius;z<=radius;z++){for(var x=-radius;x<=radius;x++){
        let c=clamp(anchor+vec2i(x,z),vec2i(0),dims-1);
        let index=c.y*dims.x+c.x;
        if bed[index].y<=0.5{continue;}
        if cubic {
            if !linked_path(anchor,c,dims,true)||!linked_path(anchor,c,dims,false){continue;}
        } else if !linked(anchor,c,dims){continue;}
        var hm=vec2f(0);
        if reconstruction==2.0{hm=textureLoad(smoothed,c,0).xy;}
        else{hm=textureLoad(field,c,0).xy;}
        let value=vec4f(hm,bed[index].x,bed[index].x+hm.x);
        bounds.lower=min(bounds.lower,value);bounds.upper=max(bounds.upper,value);
    }}
    return bounds;
}
