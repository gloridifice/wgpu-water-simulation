//! GPU checks for production NSH coverage, transformed micro normals and texture data.
use crate::ocean::{
    foam_texture::FoamTextures,
    gpu::{Error, Gpu},
};

const DENSITIES: usize = 65;
const FIXTURES: usize = 9;
const WEIGHTS: usize = DENSITIES * FIXTURES * 3;
const NORMALS: usize = 8;
const SAMPLE_SIDE: usize = 16;
const SAMPLES: usize = SAMPLE_SIDE * SAMPLE_SIDE;
const COUNT: usize = WEIGHTS + NORMALS + 3 * 3 * SAMPLES;

pub(super) fn verify(gpu: &Gpu) -> Result<(), Error> {
    let textures = FoamTextures::new(gpu);
    // The complete production module is used so the probe exercises the actual
    // helpers, not a second implementation of the material in a test shader.
    let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("NSH material and texture probe"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "{}\n{}\n{}\n{}\n{}\n{}\n{}",
                include_str!("optics.wgsl"),
                include_str!("sky.wgsl"),
                include_str!("skylight.wgsl"),
                include_str!("render.wgsl"),
                include_str!("shore.wgsl"),
                include_str!("shore_read.wgsl"),
                r#"
@group(1) @binding(0) var<storage, read_write> nsh_results: array<vec4<f32>>;
@compute @workgroup_size(64) fn nsh_probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let index=id.x;
    if index<1755u {
        let amount=f32(index%65u)/64.0;
        let fixture=(index/65u)%9u;
        let opacity=f32(index/(65u*9u))*0.5;
        let weights=nsh_weights(amount,f32(fixture%3u)*0.5,f32(fixture/3u)*0.5,0.04,0.06,0.35,opacity);
        nsh_results[index]=vec4(weights,0.0,0.0);
        return;
    }
    if index<1763u {
        let normal_case=index-1755u;
        var base=vec3(0.0,1.0,0.0);
        var slope=vec2(0.3,-0.2);
        var jacobian=vec4(1.0,0.0,0.0,1.0);
        var strength=1.0;
        if normal_case==0u { slope=vec2(0.0); }
        if normal_case==1u { base=normalize(vec3(0.3,1.0,-0.2)); slope=vec2(0.0); }
        if normal_case==3u { jacobian=vec4(0.0,1.0,-1.0,0.0); }
        if normal_case==4u { base=normalize(vec3(-0.2,1.0,0.4)); strength=0.0; }
        if normal_case==5u { jacobian=vec4(2.0,0.0,0.0,0.5); }
        if normal_case==6u { slope=vec2(1000.0,-1000.0); strength=3.0; }
        if normal_case==7u { slope=vec2(0.0,0.5); jacobian=vec4(1.0,0.0,0.0,-1.0); }
        nsh_results[index]=vec4(nsh_normal(base,slope,jacobian,strength),1.0);
        return;
    }
    let sample=index-1763u;
    if sample>=2304u { return; }
    let layer=i32(sample/(3u*256u));
    let kind=(sample/256u)%3u;
    let point=sample%256u;
    let dimensions=textureDimensions(foam_nsh,0);
    let pixel=vec2(point%16u,point/16u)*dimensions/16u+dimensions/32u;
    var uv=(vec2<f32>(pixel)+vec2(0.5))/vec2<f32>(dimensions);
    var lod=0.0;
    if kind==1u { uv+=vec2(2.0,-3.0); }
    if kind==2u { lod=f32(textureNumLevels(foam_nsh)-1u); }
    nsh_results[index]=textureSampleLevel(foam_nsh,foam_nsh_sampler,uv,layer,lod);
}
"#,
            )
            .into(),
        ),
    });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("NSH material probe"),
            layout: None,
            module: &shader,
            entry_point: Some("nsh_probe"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bytes = (COUNT * 16) as u64;
    let output = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("NSH material probe results"),
        size: bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let sampled = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("NSH probe textures"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 10,
                resource: wgpu::BindingResource::TextureView(&textures.view),
            },
            wgpu::BindGroupEntry {
                binding: 11,
                resource: wgpu::BindingResource::Sampler(&textures.sampler),
            },
        ],
    });
    let results = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("NSH probe output"),
        layout: &pipeline.get_bind_group_layout(1),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: output.as_entire_binding(),
        }],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &sampled, &[]);
        pass.set_bind_group(1, &results, &[]);
        pass.dispatch_workgroups((COUNT as u32).div_ceil(64), 1, 1);
    }
    gpu.queue.submit([encoder.finish()]);
    let data = gpu.read_buffer(&output, bytes)?;
    let values: &[[f32; 4]] = bytemuck::cast_slice(&data);
    if values.iter().flatten().any(|v| !v.is_finite()) {
        return Err("NSH material returned a non-finite value".into());
    }
    verify_weights(values)?;
    verify_normals(&values[WEIGHTS..WEIGHTS + NORMALS])?;
    verify_textures(&values[WEIGHTS + NORMALS..])?;
    println!(
        "NSH GPU: zero/monotone bounded coverage, disjoint thin/thick layers, transformed unit normals, 3 linear RGBA layers/repeat/mip means OK"
    );
    Ok(())
}

fn verify_weights(values: &[[f32; 4]]) -> Result<(), Error> {
    let at = |opacity: usize, fixture: usize, density: usize| {
        values[(opacity * FIXTURES + fixture) * DENSITIES + density]
    };
    let mut found_thin = false;
    let mut found_thick = false;
    for opacity in 0..3 {
        for fixture in 0..FIXTURES {
            let mut previous = [0.0_f32; 2];
            for density in 0..DENSITIES {
                let current = at(opacity, fixture, density);
                let total = current[0] + current[1];
                if current[..2].iter().any(|v| *v < -1e-6)
                    || total > 1.0 + 1e-6
                    || (density == 0 && total > 1e-6)
                    || (opacity == 0 && current[0].abs() > 1e-6)
                    || total + 1e-6 < previous[0]
                    || current[1] + 1e-6 < previous[1]
                {
                    return Err(format!(
                        "NSH coverage invariant failed: opacity {opacity}, fixture {fixture}, density {density}: {current:?}, previous {previous:?}"
                    ).into());
                }
                if density as f32 / 64.0 <= 0.35 && current[1] > 1e-6 {
                    return Err("NSH thick foam survived below its density threshold".into());
                }
                previous = [total, current[1]];
                found_thin |= current[0] > 0.05;
                found_thick |= current[1] > 0.05;
            }
        }
    }
    for fixture in 0..FIXTURES {
        for density in 0..DENSITIES {
            let clear = at(0, fixture, density);
            let half = at(1, fixture, density);
            let full = at(2, fixture, density);
            if (half[0] * 2.0 - full[0]).abs() > 1e-6
                || (clear[1] - full[1]).abs() > 1e-6
                || (half[1] - full[1]).abs() > 1e-6
            {
                return Err(
                    "NSH thin opacity changed thick coverage or did not scale linearly".into(),
                );
            }
            if fixture >= 3 && full[1] + 1e-6 < at(2, fixture - 3, density)[1] {
                return Err("NSH higher height reduced thick coverage".into());
            }
        }
    }
    if !found_thin || !found_thick {
        return Err("NSH coverage probe did not exercise both thin and thick foam".into());
    }
    Ok(())
}

fn verify_normals(values: &[[f32; 4]]) -> Result<(), Error> {
    let expected = [
        Some(glam::Vec3::Y),
        Some(glam::Vec3::new(0.3, 1.0, -0.2).normalize()),
        Some(glam::Vec3::new(0.3, 1.0, -0.2).normalize()),
        Some(glam::Vec3::new(-0.2, 1.0, -0.3).normalize()),
        Some(glam::Vec3::new(-0.2, 1.0, 0.4).normalize()),
        Some(glam::Vec3::new(0.6, 1.0, -0.1).normalize()),
        None,
        Some(glam::Vec3::new(0.0, 1.0, -0.5).normalize()),
    ];
    for (case, (value, expected)) in values.iter().zip(expected).enumerate() {
        let actual = glam::Vec3::from_slice(value);
        if (actual.length() - 1.0).abs() > 2e-6
            || actual.y <= 0.0
            || expected.is_some_and(|n| (actual - n).length() > 2e-6)
        {
            return Err(
                format!("NSH normal case {case}: {actual:?}, expected {expected:?}").into(),
            );
        }
    }
    Ok(())
}

fn verify_textures(values: &[[f32; 4]]) -> Result<(), Error> {
    let sources = [
        include_bytes!("../../assets/foam/baked/shorewaves_nsh.png").as_slice(),
        include_bytes!("../../assets/foam/baked/crest_foam_nsh.png").as_slice(),
        include_bytes!("../../assets/foam/baked/crest_foam2_nsh.png").as_slice(),
    ];
    for (layer, source) in sources.into_iter().enumerate() {
        let mut reader = png::Decoder::new(std::io::Cursor::new(source)).read_info()?;
        let mut pixels = vec![0; reader.output_buffer_size().ok_or("NSH PNG output size")?];
        let info = reader.next_frame(&mut pixels)?;
        if info.bit_depth != png::BitDepth::Eight || info.color_type != png::ColorType::Rgba {
            return Err("NSH probe requires RGBA8 source data".into());
        }
        let (width, height) = (info.width as usize, info.height as usize);
        let count = (width * height) as f64;
        let mut average_normal = glam::DVec3::ZERO;
        let mut means = [0.0_f64; 4];
        let (texels, remainder) = pixels[..info.buffer_size()].as_chunks::<4>();
        if !remainder.is_empty() || texels.len() != width * height {
            return Err("NSH PNG dimensions do not match its RGBA payload".into());
        }
        for pixel in texels {
            let x = pixel[0] as f64 / 127.5 - 1.0;
            let y = pixel[1] as f64 / 127.5 - 1.0;
            let z = (1.0 - x * x - y * y).max(0.0).sqrt();
            average_normal += glam::DVec3::new(x, y, z).normalize() / count;
            means[2] += pixel[2] as f64 / (255.0 * count);
            means[3] += pixel[3] as f64 / (255.0 * count);
        }
        let normal = average_normal.normalize();
        means[0] = normal.x * 0.5 + 0.5;
        means[1] = normal.y * 0.5 + 0.5;
        let source_start = layer * 3 * SAMPLES;
        for point in 0..SAMPLES {
            let x = (point % SAMPLE_SIDE) * width / SAMPLE_SIDE + width / (2 * SAMPLE_SIDE);
            let y = (point / SAMPLE_SIDE) * height / SAMPLE_SIDE + height / (2 * SAMPLE_SIDE);
            let pixel = texels[y * width + x];
            let base = values[source_start + point];
            let repeated = values[source_start + SAMPLES + point];
            let coarsest = values[source_start + 2 * SAMPLES + point];
            for channel in 0..4 {
                // RG may be renormalized when quantization places XY just
                // outside the unit disk. B/A must remain independent scalars.
                let tolerance = if channel < 2 { 2.0 / 255.0 } else { 2e-5 };
                if !(0.0..=1.0).contains(&base[channel])
                    || (base[channel] - pixel[channel] as f32 / 255.0).abs() > tolerance
                    || (base[channel] - repeated[channel]).abs() > 2e-5
                    || (coarsest[channel] as f64 - means[channel]).abs() > 0.51 / 255.0 + 1e-5
                {
                    return Err(format!(
                        "NSH layer {layer}, texel ({x},{y}), channel {channel}: base {}, repeat {}, mip {}, source {}, mean {}",
                        base[channel], repeated[channel], coarsest[channel], pixel[channel], means[channel]
                    ).into());
                }
            }
        }
        if means[2] <= 0.0 || means[2] >= 1.0 || means[3] <= 0.0 || means[3] >= 1.0 {
            return Err(format!("NSH layer {layer} lost Soft/Height variation: {means:?}").into());
        }
    }
    Ok(())
}
