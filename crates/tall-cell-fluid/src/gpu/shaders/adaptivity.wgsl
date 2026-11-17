// Optical-layer numerical stages. Each entry point uses sparse bindings for its statically used resources.
// q, integration, distance propagation and height smoothing run on the GPU; the CPU only organizes the quadtree and pressure basis.
struct Params {
    dimensions: vec4<u32>, // nx, ny, nz, cell_count
    offsets: vec4<u32>,    // u/v/w offsets, column_count
    controls: vec4<u32>,   // airborne_layers, reserved
    coefficients: vec4<f32>, // optical_scale, reserved
}

struct Metric {
    local_error: f32,
    run_error: f32,
    first: u32,
    limit: u32,
}

struct Column {
    integrated_error: f32,
    first: u32,
    limit: u32,
    single_grounded: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> interface_data: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> velocities: array<f32>;
@group(0) @binding(3) var<storage, read_write> metrics: array<Metric>;
@group(0) @binding(4) var<storage, read_write> columns: array<Column>;
@group(0) @binding(5) var<storage, read> leaf_widths: array<u32>;
@group(0) @binding(6) var<storage, read> work_in: array<i32>;
@group(0) @binding(7) var<storage, read_write> work_out: array<i32>;

fn cell_index(p: vec3<u32>) -> u32 {
    return p.x + params.dimensions.x * (p.y + params.dimensions.y * p.z);
}

fn cell_coord(index: u32) -> vec3<u32> {
    return vec3<u32>(
        index % params.dimensions.x,
        (index / params.dimensions.x) % params.dimensions.y,
        index / (params.dimensions.x * params.dimensions.y),
    );
}

fn in_bounds(p: vec3<i32>) -> bool {
    return all(p >= vec3<i32>(0)) && all(p < vec3<i32>(params.dimensions.xyz));
}

fn is_fluid(p: vec3<u32>) -> bool {
    let value = interface_data[cell_index(p)];
    return value.x < 0.0 && value.y >= 0.0;
}

fn grounded(x: u32, z: u32, first: u32) -> bool {
    if first == 0u { return true; }
    return interface_data[cell_index(vec3<u32>(x, first - 1u, z))].y < 0.0;
}

fn face_velocity(component: u32, p: vec3<u32>) -> f32 {
    var dims = params.dimensions.xyz;
    dims[component] += 1u;
    return velocities[params.offsets[component] + p.x + dims.x * (p.y + dims.y * p.z)];
}

// Each thread handles one column, processing continuous liquid segments separated by solid or air to avoid fitting across segments.
@compute @workgroup_size(64)
fn evaluate_metrics(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let column = invocation.x;
    if column >= params.offsets.w { return; }
    let x = column % params.dimensions.x;
    let z = column / params.dimensions.x;
    let ny = params.dimensions.y;
    var y = 0u;
    var run_count = 0u;
    var column_result = Column(0.0, 0u, 0u, 0u);
    for (var clear_y = 0u; clear_y < ny; clear_y += 1u) {
        metrics[cell_index(vec3<u32>(x, clear_y, z))] = Metric(0.0, 0.0, 0u, 0u);
    }
    loop {
        if y >= ny { break; }
        if !is_fluid(vec3<u32>(x, y, z)) { y += 1u; continue; }
        let first = y;
        loop {
            if y >= ny { break; }
            if !is_fluid(vec3<u32>(x, y, z)) { break; }
            y += 1u;
        }
        let limit = y;
        let count = f32(limit - first);
        let center = f32(first + limit - 1u) * 0.5;
        var variance = 0.0;
        for (var j = first; j < limit; j += 1u) {
            let offset = f32(j) - center;
            variance += offset * offset;
        }
        // Each horizontal component has two sampling columns (left/right or front/back); fit each column by linear least squares.
        for (var horizontal = 0u; horizontal < 2u; horizontal += 1u) {
            let component = horizontal * 2u;
            for (var side = 0u; side < 2u; side += 1u) {
                var mean = 0.0;
                for (var j = first; j < limit; j += 1u) {
                    var face = vec3<u32>(x, j, z);
                    face[component] += side;
                    mean += face_velocity(component, face);
                }
                mean /= count;
                var covariance = 0.0;
                for (var j = first; j < limit; j += 1u) {
                    var face = vec3<u32>(x, j, z);
                    face[component] += side;
                    covariance += (f32(j) - center) * (face_velocity(component, face) - mean);
                }
                var slope = 0.0;
                if variance > 0.0 { slope = covariance / variance; }
                for (var j = first; j < limit; j += 1u) {
                    var face = vec3<u32>(x, j, z);
                    face[component] += side;
                    let reconstructed = mean + slope * (f32(j) - center);
                    metrics[cell_index(vec3<u32>(x, j, z))].local_error +=
                        abs(reconstructed - face_velocity(component, face));
                }
            }
        }
        var vertical_mean = 0.0;
        for (var j = first; j <= limit; j += 1u) {
            vertical_mean += face_velocity(1u, vec3<u32>(x, j, z));
        }
        vertical_mean /= count + 1.0;
        var total = 0.0;
        for (var j = first; j < limit; j += 1u) {
            let index = cell_index(vec3<u32>(x, j, z));
            metrics[index].local_error +=
                abs(face_velocity(1u, vec3<u32>(x, j, z)) - vertical_mean)
                + abs(face_velocity(1u, vec3<u32>(x, j + 1u, z)) - vertical_mean);
            total += metrics[index].local_error;
        }
        // Use the number of layers in the whole domain, not the current liquid segment.
        total /= f32(ny);
        for (var j = first; j < limit; j += 1u) {
            let index = cell_index(vec3<u32>(x, j, z));
            metrics[index].run_error = total;
            metrics[index].first = first;
            metrics[index].limit = limit;
        }
        run_count += 1u;
        column_result.integrated_error += total;
        column_result.first = first;
        column_result.limit = limit;
        column_result.single_grounded = select(0u, 1u, grounded(x, z, first));
    }
    if run_count != 1u { column_result.single_grounded = 0u; }
    columns[column] = column_result;
}

fn touches_air(p: vec3<u32>) -> bool {
    for (var axis = 0u; axis < 3u; axis += 1u) {
        for (var side = 0u; side < 2u; side += 1u) {
            var neighbor = vec3<i32>(p);
            neighbor[axis] += select(-1, 1, side == 1u);
            if in_bounds(neighbor) {
                let value = interface_data[cell_index(vec3<u32>(neighbor))];
                if value.x >= 0.0 && value.y >= 0.0 { return true; }
            }
        }
    }
    return false;
}

@compute @workgroup_size(64)
fn seed_optical_layer(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let index = invocation.x;
    if index >= params.dimensions.w { return; }
    work_out[index] = -1;
    let metric = metrics[index];
    if metric.limit <= metric.first { return; }
    let p = cell_coord(index);
    if !touches_air(p) { return; }
    let column = p.x + params.dimensions.x * p.z;
    var radius = params.controls.x;
    if grounded(p.x, p.z, metric.first) {
        let lower = min(max(2u * leaf_widths[column], 4u), params.dimensions.y);
        let upper = max(params.dimensions.y / 8u, lower);
        // Compare the saturation limit first to avoid overflow when multiplying two finite f32 values.
        var scaled = 0.0;
        if params.coefficients.x > 0.0 {
            if metric.run_error >= f32(upper) / params.coefficients.x {
                scaled = f32(upper);
            } else {
                scaled = params.coefficients.x * metric.run_error;
            }
        }
        radius = u32(ceil(clamp(scaled, f32(lower), f32(upper))));
    }
    work_out[index] = i32(radius);
}

// Each neighborhood propagation travels one cell; separate reads and writes and repeat for the maximum radius to obtain the full union of L1 balls.
@compute @workgroup_size(64)
fn dilate_optical_layer(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let index = invocation.x;
    if index >= params.dimensions.w { return; }
    let p = cell_coord(index);
    var remaining = work_in[index];
    for (var axis = 0u; axis < 3u; axis += 1u) {
        for (var side = 0u; side < 2u; side += 1u) {
            var neighbor = vec3<i32>(p);
            neighbor[axis] += select(-1, 1, side == 1u);
            if in_bounds(neighbor) {
                remaining = max(remaining, work_in[cell_index(vec3<u32>(neighbor))] - 1);
            }
        }
    }
    work_out[index] = remaining;
}

@compute @workgroup_size(64)
fn initialize_heights(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let column = invocation.x;
    if column >= params.offsets.w { return; }
    work_out[column] = -1;
    let description = columns[column];
    if description.single_grounded == 0u { return; }
    let x = column % params.dimensions.x;
    let z = column / params.dimensions.x;
    var height = description.limit;
    for (var y = description.first; y < description.limit; y += 1u) {
        if work_in[cell_index(vec3<u32>(x, y, z))] >= 0 {
            height = y;
            break;
        }
    }
    work_out[column] = i32(height);
}

@compute @workgroup_size(64)
fn smooth_heights(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let column = invocation.x;
    if column >= params.offsets.w { return; }
    var height = work_in[column];
    if height < 0 { work_out[column] = -1; return; }
    let description = columns[column];
    let p = vec2<i32>(i32(column % params.dimensions.x), i32(column / params.dimensions.x));
    let size = vec2<i32>(i32(params.dimensions.x), i32(params.dimensions.z));
    for (var axis = 0u; axis < 2u; axis += 1u) {
        for (var side = 0u; side < 2u; side += 1u) {
            var neighbor = p;
            neighbor[axis] += select(-1, 1, side == 1u);
            if all(neighbor >= vec2<i32>(0)) && all(neighbor < size) {
                let other_index = u32(neighbor.x) + params.dimensions.x * u32(neighbor.y);
                let other_height = work_in[other_index];
                let other = columns[other_index];
                if other_height >= 0 && description.first < other.limit && other.first < description.limit {
                    height = min(height, max(other_height + 1, i32(description.first)));
                }
            }
        }
    }
    work_out[column] = height;
}

@compute @workgroup_size(64)
fn finalize_optical_layer(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let index = invocation.x;
    if index >= params.dimensions.w { return; }
    let p = cell_coord(index);
    let column = p.x + params.dimensions.x * p.z;
    let height = work_in[column];
    if height >= 0 && p.y >= u32(height) && p.y < columns[column].limit {
        work_out[index] = max(work_out[index], 0);
    }
}
