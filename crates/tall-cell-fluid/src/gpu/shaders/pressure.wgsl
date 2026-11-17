// One binding layout covers all pressure stages; mode is changed only by the dispatcher.
struct Params {
    grid: vec4<u32>,             // nx, ny, nz, fine cell count
    counts: vec4<u32>,           // dofs, velocity count, fluid count, mode
    meta0: vec4<u32>,            // flags, basis indices, transpose rows, transpose cells
    meta1: vec4<u32>,            // matrix rows, matrix columns, pinned, reserved
    values0: vec4<u32>,          // face inverse distance, basis weights, transpose weights, matrix coefficients
    values1: vec4<u32>,          // diagonal, x-face count, y-face count, reserved
    scales: vec4<f32>,           // dx, dt/rho, rho/dt, reserved
};
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> metadata: array<u32>;
@group(0) @binding(2) var<storage, read> coefficients: array<f32>;
@group(0) @binding(3) var<storage, read_write> velocity: array<f32>;
@group(0) @binding(4) var<storage, read_write> fine_pressure: array<f32>;
// x, b, r, z, p, A*p, each of length dofs.
@group(0) @binding(5) var<storage, read_write> vectors: array<f32>;
// rr, rz, pAp, bb, previous_rz, divergence_rms, divergence_max。
@group(0) @binding(6) var<storage, read_write> stats: array<f32>;
var<workgroup> reduction: array<vec4<f32>, 256>;

fn cell_index(p: vec3<u32>) -> u32 {
    return p.x + params.grid.x * (p.y + params.grid.y * p.z);
}
fn cell_coord(i: u32) -> vec3<u32> {
    return vec3<u32>(i % params.grid.x, (i / params.grid.x) % params.grid.y,
                     i / (params.grid.x * params.grid.y));
}
fn face_coord(i: u32) -> vec4<u32> {
    var axis = 0u;
    var local = i;
    if i >= params.values1.y + params.values1.z {
        axis = 2u;
        local -= params.values1.y + params.values1.z;
    } else if i >= params.values1.y {
        axis = 1u;
        local -= params.values1.y;
    }
    var size = params.grid.xyz;
    size[axis] += 1u;
    return vec4<u32>(local % size.x, (local / size.x) % size.y,
                     local / (size.x * size.y), axis);
}
fn blocked(face: vec4<u32>) -> bool {
    let axis = face.w;
    if face[axis] == 0u || face[axis] == params.grid[axis] { return true; }
    var left = face.xyz;
    left[axis] -= 1u;
    return metadata[params.meta0.x + cell_index(left)] == 2u
        || metadata[params.meta0.x + cell_index(face.xyz)] == 2u;
}
fn divergence(cell: u32) -> f32 {
    let p = cell_coord(cell);
    let x = p.x + (params.grid.x + 1u) * (p.y + params.grid.y * p.z);
    let y = params.values1.y + p.x + params.grid.x * (p.y + (params.grid.y + 1u) * p.z);
    let z = params.values1.y + params.values1.z + p.x + params.grid.x * (p.y + params.grid.y * p.z);
    return (velocity[x + 1u] - velocity[x]
        + velocity[y + params.grid.x] - velocity[y]
        + velocity[z + params.grid.x * params.grid.y] - velocity[z]) / params.scales.x;
}
fn matvec(row: u32, source: u32) -> f32 {
    let first = metadata[params.meta1.x + row];
    let last = metadata[params.meta1.x + row + 1u];
    var sum = 0.0;
    for (var entry = first; entry < last; entry += 1u) {
        let column = metadata[params.meta1.y + entry];
        sum += coefficients[params.values0.w + entry] * vectors[source + column];
    }
    return sum;
}

@compute @workgroup_size(256)
fn pressure_main(@builtin(global_invocation_id) gid: vec3<u32>,
                 @builtin(local_invocation_id) lid: vec3<u32>) {
    let i = gid.x;
    let n = params.counts.x;
    let mode = params.counts.w;
    // Reduction dispatches one workgroup; each thread strides across the array before reducing in shared memory.
    if mode == 4u {
        var sums = vec4<f32>(0.0);
        for (var k = lid.x; k < n; k += 256u) {
            let r = vectors[2u * n + k];
            let z = vectors[3u * n + k];
            let p = vectors[4u * n + k];
            let ap = vectors[5u * n + k];
            let b = vectors[n + k];
            sums += vec4<f32>(r*r, r*z, p*ap, b*b);
        }
        reduction[lid.x] = sums;
        workgroupBarrier();
        for (var stride = 128u; stride > 0u; stride /= 2u) {
            if lid.x < stride { reduction[lid.x] += reduction[lid.x + stride]; }
            workgroupBarrier();
        }
        if lid.x == 0u {
            stats[0] = reduction[0].x;
            stats[1] = reduction[0].y;
            stats[2] = reduction[0].z;
            stats[3] = reduction[0].w;
        }
    } else if mode == 10u {
        var sums = vec4<f32>(0.0);
        for (var k = lid.x; k < params.grid.w; k += 256u) {
            if metadata[params.meta0.x + k] == 1u {
                let d = divergence(k);
                sums.x += d*d;
                sums.y = max(sums.y, abs(d));
            }
        }
        reduction[lid.x] = sums;
        workgroupBarrier();
        for (var stride = 128u; stride > 0u; stride /= 2u) {
            if lid.x < stride {
                reduction[lid.x].x += reduction[lid.x + stride].x;
                reduction[lid.x].y = max(reduction[lid.x].y, reduction[lid.x + stride].y);
            }
            workgroupBarrier();
        }
        if lid.x == 0u {
            stats[5] = sqrt(reduction[0].x / f32(max(params.counts.z, 1u)));
            stats[6] = reduction[0].y;
        }
    } else if mode == 0u {
        if i < params.counts.y && blocked(face_coord(i)) { velocity[i] = 0.0; }
    } else if mode == 1u && i < n {
        var b = 0.0;
        if metadata[params.meta1.z + i] == 0u {
            let first = metadata[params.meta0.z + i];
            let last = metadata[params.meta0.z + i + 1u];
            for (var entry = first; entry < last; entry += 1u) {
                let cell = metadata[params.meta0.w + entry];
                b -= coefficients[params.values0.z + entry] * divergence(cell) * params.scales.z;
            }
        }
        vectors[i] = 0.0;
        vectors[n + i] = b;
        vectors[2u*n + i] = b;
        let z = b / coefficients[params.values1.x + i];
        vectors[3u*n + i] = z;
        vectors[4u*n + i] = z;
        vectors[5u*n + i] = 0.0;
    } else if mode == 2u && i < n {
        vectors[5u*n + i] = matvec(i, 4u*n);
    } else if mode == 3u && i < n {
        let alpha = stats[1] / stats[2];
        vectors[i] += alpha * vectors[4u*n + i];
        vectors[2u*n + i] -= alpha * vectors[5u*n + i];
        vectors[3u*n + i] = vectors[2u*n + i] / coefficients[params.values1.x + i];
        if i == 0u { stats[4] = stats[1]; }
    } else if mode == 5u && i < n {
        let beta = stats[1] / stats[4];
        vectors[4u*n + i] = vectors[3u*n + i] + beta * vectors[4u*n + i];
    } else if mode == 6u && i < n {
        vectors[5u*n + i] = matvec(i, 0u);
    } else if mode == 7u && i < n {
        let r = vectors[n + i] - vectors[5u*n + i];
        let z = r / coefficients[params.values1.x + i];
        vectors[2u*n + i] = r;
        vectors[3u*n + i] = z;
        vectors[4u*n + i] = z;
    } else if mode == 8u && i < params.grid.w {
        var pressure = 0.0;
        for (var k = 0u; k < 2u; k += 1u) {
            let weight = coefficients[params.values0.y + 2u*i + k];
            if weight != 0.0 {
                let index = metadata[params.meta0.y + 2u*i + k];
                pressure += weight * vectors[index];
            }
        }
        fine_pressure[i] = pressure;
    } else if mode == 9u && i < params.counts.y {
        let face = face_coord(i);
        if blocked(face) {
            velocity[i] = 0.0;
        } else {
            let inverse_distance = coefficients[params.values0.x + i];
            if inverse_distance != 0.0 {
                var left = face.xyz;
                left[face.w] -= 1u;
                let gradient = (fine_pressure[cell_index(face.xyz)] - fine_pressure[cell_index(left)]) * inverse_distance;
                velocity[i] -= params.scales.y * gradient;
            }
        }
    }
}
