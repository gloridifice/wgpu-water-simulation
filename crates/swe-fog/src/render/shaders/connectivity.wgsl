// Shared by render reconstruction and its prefilter. Each local stencil reaches
// at most two cells away; no tap may skip a blocked face or turn around a corner.
fn linked(a:vec2i,b:vec2i,dims:vec2i)->bool {
    if a.x!=b.x {let x=max(a.x,b.x);if masks[a.y*(dims.x+1)+x]==0u{return false;}}
    if a.y!=b.y {let offset=(dims.x+1)*dims.y;let z=max(a.y,b.y);if masks[offset+z*dims.x+b.x]==0u{return false;}}return true;
}
fn linked_path(a:vec2i,b:vec2i,dims:vec2i,x_first:bool)->bool {
    var c=a;
    for(var order=0;order<2;order++){
        let axis=select(1-order,order,x_first);
        for(var n=0;n<2;n++){
            if c[axis]==b[axis]{break;}
            var next=c;next[axis]+=select(-1,1,b[axis]>c[axis]);
            if axis==0{if masks[c.y*(dims.x+1)+max(c.x,next.x)]==0u{return false;}}
            else{if masks[(dims.x+1)*dims.y+max(c.y,next.y)*dims.x+c.x]==0u{return false;}}
            c=next;
        }
    }
    return all(c==b);
}
