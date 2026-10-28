// 按单元中心采样的六四面体等值面。水内 phi < 0，零样本归水外。
struct Parameters {
    dimensions: vec4<u32>,
    origin_spacing: vec4<f32>,
    capacity: vec4<u32>,
}

struct Vertex {
    position: vec4<f32>,
    normal: vec4<f32>,
}

struct Counters {
    triangles: atomic<u32>,
    unused0: u32,
    unused1: u32,
    unused2: u32,
}

@group(0) @binding(0) var<uniform> params: Parameters;
@group(0) @binding(1) var<storage, read> phi: array<f32>;
@group(0) @binding(2) var<storage, read_write> output_vertices: array<Vertex>;
@group(0) @binding(3) var<storage, read_write> counters: Counters;

fn index(p: vec3<u32>) -> u32 {
    return p.x + params.dimensions.x * (p.y + params.dimensions.y * p.z);
}

fn corner_offset(corner: u32) -> vec3<u32> {
    return vec3<u32>(corner & 1u, (corner >> 1u) & 1u, corner >> 2u);
}

fn safe_normalize(v: vec3<f32>) -> vec3<f32> {
    let scale = max(abs(v.x), max(abs(v.y), abs(v.z)));
    if scale == 0.0 {
        return vec3<f32>(0.0);
    }
    let scaled = v / scale;
    return scaled / sqrt(dot(scaled, scaled));
}

fn gradient(p: vec3<u32>) -> vec3<f32> {
    var result = vec3<f32>(0.0);
    for (var axis = 0u; axis < 3u; axis++) {
        var low = p;
        var high = p;
        low[axis] = select(0u, p[axis] - 1u, p[axis] > 0u);
        high[axis] = min(p[axis] + 1u, params.dimensions[axis] - 1u);
        result[axis] = (phi[index(high)] - phi[index(low)]) / f32(high[axis] - low[axis]);
    }
    return result;
}

fn intersection(base: vec3<u32>, ca: u32, cb: u32) -> Vertex {
    var a = base + corner_offset(ca);
    var b = base + corner_offset(cb);
    // 固定边的采样次序，避免相邻四面体以不同端点顺序插值产生接缝。
    if index(a) > index(b) {
        let old_a = a;
        a = b;
        b = old_a;
    }
    let pa = phi[index(a)];
    let pb = phi[index(b)];
    let t = abs(pa) / (abs(pa) + abs(pb));
    let coordinate = mix(vec3<f32>(a), vec3<f32>(b), t) + vec3<f32>(0.5);
    var vertex: Vertex;
    vertex.position = vec4<f32>(params.origin_spacing.xyz + coordinate * params.origin_spacing.w, 1.0);
    vertex.normal = vec4<f32>(safe_normalize(mix(gradient(a), gradient(b), t)), 0.0);
    return vertex;
}

fn triangle(va: Vertex, vb: Vertex, vc: Vertex, outward: vec3<f32>) {
    var a = va;
    var b = vb;
    var c = vc;
    let edge1 = b.position.xyz - a.position.xyz;
    let edge2 = c.position.xyz - a.position.xyz;
    let scale = max(max(abs(edge1.x), max(abs(edge1.y), abs(edge1.z))),
                    max(abs(edge2.x), max(abs(edge2.y), abs(edge2.z))));
    if scale == 0.0 {
        return;
    }
    var normal = safe_normalize(cross(edge1 / scale, edge2 / scale));
    if all(normal == vec3<f32>(0.0)) {
        return;
    }
    if dot(normal, outward) < 0.0 {
        let old_b = b;
        b = c;
        c = old_b;
        normal = -normal;
    }
    if all(a.normal.xyz == vec3<f32>(0.0)) { a.normal = vec4<f32>(normal, 0.0); }
    if all(b.normal.xyz == vec3<f32>(0.0)) { b.normal = vec4<f32>(normal, 0.0); }
    if all(c.normal.xyz == vec3<f32>(0.0)) { c.normal = vec4<f32>(normal, 0.0); }
    let triangle_index = atomicAdd(&counters.triangles, 1u);
    if triangle_index >= params.capacity.x {
        return;
    }
    let offset = 3u * triangle_index;
    output_vertices[offset] = a;
    output_vertices[offset + 1u] = b;
    output_vertices[offset + 2u] = c;
}

fn tetrahedron(base: vec3<u32>, corners: vec4<u32>) {
    var inside: array<u32, 4>;
    var outside: array<u32, 4>;
    var ni = 0u;
    var no = 0u;
    var inside_center = vec3<f32>(0.0);
    var outside_center = vec3<f32>(0.0);
    for (var i = 0u; i < 4u; i++) {
        let corner = corners[i];
        let offset = corner_offset(corner);
        if phi[index(base + offset)] < 0.0 {
            inside[ni] = corner;
            ni++;
            inside_center += vec3<f32>(offset);
        } else {
            outside[no] = corner;
            no++;
            outside_center += vec3<f32>(offset);
        }
    }
    if ni == 0u || no == 0u {
        return;
    }
    let outward = outside_center / f32(no) - inside_center / f32(ni);
    if ni == 1u {
        triangle(intersection(base, inside[0], outside[0]),
                 intersection(base, inside[0], outside[1]),
                 intersection(base, inside[0], outside[2]), outward);
    } else if no == 1u {
        triangle(intersection(base, outside[0], inside[0]),
                 intersection(base, outside[0], inside[1]),
                 intersection(base, outside[0], inside[2]), outward);
    } else {
        let a = intersection(base, inside[0], outside[0]);
        let b = intersection(base, inside[0], outside[1]);
        let c = intersection(base, inside[1], outside[1]);
        let d = intersection(base, inside[1], outside[0]);
        triangle(a, b, c, outward);
        triangle(a, c, d, outward);
    }
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params.dimensions.w {
        return;
    }
    let cube_size = params.dimensions.xyz - vec3<u32>(1u);
    let base = vec3<u32>(id.x % cube_size.x,
                          (id.x / cube_size.x) % cube_size.y,
                          id.x / (cube_size.x * cube_size.y));
    var negative_count = 0u;
    for (var corner = 0u; corner < 8u; corner++) {
        if phi[index(base + corner_offset(corner))] < 0.0 {
            negative_count++;
        }
    }
    if negative_count == 0u || negative_count == 8u {
        return;
    }
    tetrahedron(base, vec4<u32>(0u, 1u, 3u, 7u));
    tetrahedron(base, vec4<u32>(0u, 3u, 2u, 7u));
    tetrahedron(base, vec4<u32>(0u, 2u, 6u, 7u));
    tetrahedron(base, vec4<u32>(0u, 6u, 4u, 7u));
    tetrahedron(base, vec4<u32>(0u, 4u, 5u, 7u));
    tetrahedron(base, vec4<u32>(0u, 5u, 1u, 7u));
}
