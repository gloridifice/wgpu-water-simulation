// Vertex reconstruction is separate from the linear tetrahedron integrator.
@group(0) @binding(0) var<uniform> camera:RasterCamera;
@group(0) @binding(2) var field:texture_2d<f32>;
@group(0) @binding(3) var<storage,read> bed:array<vec4f>;
@group(0) @binding(4) var smoothed:texture_2d<f32>;
@group(0) @binding(5) var<storage,read_write> tets:array<TetData>;
@group(0) @binding(7) var<storage,read> masks:array<u32>;
fn cubic_weights(t:f32,catmull:bool)->vec4f {
    let t2=t*t;let t3=t2*t;
    if catmull{return vec4f(-0.5*t+t2-0.5*t3,1.0-2.5*t2+1.5*t3,0.5*t+2.0*t2-1.5*t3,-0.5*t2+0.5*t3);}
    return vec4f((1.0-t)*(1.0-t)*(1.0-t),4.0-6.0*t2+3.0*t3,1.0+3.0*t+3.0*t2-3.0*t3,t3)/6.0;
}
fn vertex_value(position:vec2f,anchor:vec2i,dims:vec2i)->vec3f {
    let base=vec2i(floor(position-vec2f(0.5)));let fraction=fract(position-vec2f(0.5));
    var cells:array<vec2i,4>;var valid:array<bool,4>;var connected:array<bool,4>;
    for(var i=0u;i<4u;i++){
        let c=base+vec2i(i32(i&1u),i32(i>>1u));cells[i]=c;
        valid[i]=all(c>=vec2i(0))&&all(c<dims);
        if valid[i]{valid[i]=bed[c.y*dims.x+c.x].y>0.5;}
        connected[i]=valid[i]&&all(c==anchor);
    }
    for(var iteration=0u;iteration<3u;iteration++){
        for(var i=0u;i<4u;i++){for(var j=0u;j<4u;j++){
            if connected[i]&&valid[j]&&(abs(cells[i].x-cells[j].x)+abs(cells[i].y-cells[j].y))==1&&linked(cells[i],cells[j],dims){connected[j]=true;}
        }}
    }
    var f=fraction;if camera.player.w==3.0{f=f*f*(vec2f(3)-2.0*f);}
    var hm=vec2f(0);var weight=0.0;var bottom=0.0;var bottom_weight=0.0;
    var canonical=anchor;var found=false;
    for(var i=0u;i<4u;i++){if connected[i]{
        let c=cells[i];if !found{canonical=c;found=true;}
        let w=select(1.0-f.x,f.x,(i&1u)!=0u)*select(1.0-f.y,f.y,(i&2u)!=0u);
        let bw=select(1.0-fraction.x,fraction.x,(i&1u)!=0u)*select(1.0-fraction.y,fraction.y,(i&2u)!=0u);
        var q=textureLoad(field,c,0).xy;if camera.player.w==2.0{q=textureLoad(smoothed,c,0).xy;}
        hm+=w*q;weight+=w;bottom+=bw*bed[c.y*dims.x+c.x].x;bottom_weight+=bw;
    }}
    hm/=max(weight,1e-6);bottom/=max(bottom_weight,1e-6);
    if camera.player.w!=1.0&&camera.player.w!=4.0{return vec3f(hm,bottom);}
    let catmull=camera.player.w==4.0;let wx=cubic_weights(fraction.x,catmull);let wz=cubic_weights(fraction.y,catmull);
    var sum=vec2f(0);var total=0.0;var low=vec2f(1e30);var high=vec2f(0);
    for(var z=0;z<4;z++){for(var x=0;x<4;x++){
        let c=clamp(base+vec2i(x-1,z-1),vec2i(0),dims-vec2i(1));
        if bed[c.y*dims.x+c.x].y<=0.5{continue;}
        var core=false;var allowed=false;
        for(var i=0u;i<4u;i++){if all(c==cells[i]){core=true;allowed=allowed||connected[i];}}
        // A canonical anchor makes shared vertices independent of which cell
        // generated them. Peripheral taps cannot cross walls via either L path.
        if !core{allowed=linked_path(canonical,c,dims,true)&&linked_path(canonical,c,dims,false);}
        if !allowed{continue;}
        let q=textureLoad(field,c,0).xy;let w=wx[x]*wz[z];
        sum+=w*q;total+=w;low=min(low,q);high=max(high,q);
    }}
    if total>1e-6{
        hm=sum/total;
        // Negative Catmull weights must not create negative mass/depth or new
        // extrema, especially after a barrier removes part of the stencil.
        if catmull{hm=clamp(hm,low,high);}
    }
    return vec3f(max(hm,vec2f(0)),bottom);
}
@compute @workgroup_size(8,8)
fn prepare_tets(@builtin(global_invocation_id) id:vec3u) {
    let dims=textureDimensions(field);if any(id.xy>=dims){return;}
    let index=id.y*dims.x+id.x;let spacing=camera.domain.zw/vec2f(dims);
    let detail=camera.viewport.w>0.5;let triangles=select(2u,4u,detail);let count=triangles*3u;
    var vertices:array<vec4f,10>;
    if bed[index].y>0.5 {
        for(var point=0u;point<select(4u,5u,detail);point++){
            var offset=vec2f(f32(point&1u),f32(point>>1u));if point==4u{offset=vec2f(0.25);}
            let grid=vec2f(id.xy)+offset;let q=vertex_value(grid,vec2i(id.xy),vec2i(dims));let xz=camera.domain.xy+grid*spacing;
            var h=0.0;var density=0.0;if q.x>0.0001&&q.y>0.0{h=q.x;density=2.0*q.y/h;}
            vertices[point*2u]=vec4f(xz.x,q.z,xz.y,density);
            vertices[point*2u+1u]=vec4f(xz.x,q.z+h,xz.y,0.0);
        }
    }
    let basic=array<vec3u,2>(vec3u(0,1,3),vec3u(0,2,3));
    let enhanced=array<vec3u,4>(vec3u(0,1,4),vec3u(0,2,4),vec3u(1,3,4),vec3u(2,3,4));
    for(var triangle=0u;triangle<triangles;triangle++){
        var p=basic[min(triangle,1u)];if detail{p=enhanced[triangle];}p*=2u;
        // Sorted triangle vertices give identical diagonals on all shared
        // vertical faces, including interfaces between neighbouring cells.
        let topology=array<vec4u,3>(vec4u(p.x,p.y,p.z,p.z+1u),vec4u(p.x,p.y,p.y+1u,p.z+1u),vec4u(p.x,p.x+1u,p.y+1u,p.z+1u));
        for(var i=0u;i<3u;i++){
            let ids=topology[i];var tet=TetData(array<vec4f,4>(vertices[ids.x],vertices[ids.y],vertices[ids.z],vertices[ids.w]));
            let determinant=dot(tet.p[1].xyz-tet.p[0].xyz,cross(tet.p[2].xyz-tet.p[0].xyz,tet.p[3].xyz-tet.p[0].xyz));
            if determinant<0.0{let swap=tet.p[1];tet.p[1]=tet.p[2];tet.p[2]=swap;}
            tets[index*count+triangle*3u+i]=tet;
        }
    }
}
