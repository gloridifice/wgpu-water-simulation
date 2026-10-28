//! 独立的双精度直接 DFT 作为 GPU FFT 的数值参照；不依赖另一个 FFT 实现。
use super::{Ocean, spectrum::initial_spectrum};
use crate::ocean::{Error, gpu::context::Gpu, parameters::ocean::OceanConfig};
use std::f64::consts::TAU;

fn mul(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] * b[0] - a[1] * b[1], a[0] * b[1] + a[1] * b[0]]
}

fn reference(config: OceanConfig, time: f32) -> Vec<[f64; 2]> {
    let n = config.resolution as usize;
    let count = n * n;
    let initial = initial_spectrum(config);
    let mut frequency = vec![[0.0; 2]; 6 * count];
    for z in 0..n {
        for x in 0..n {
            let i = z * n + x;
            let other = initial[((n - z) % n) * n + (n - x) % n];
            let m = initial[i];
            let k = (f64::from(m[2]).powi(2) + f64::from(m[3]).powi(2)).sqrt();
            let angle = (9.81 * k).sqrt() * f64::from(time);
            let p = [angle.cos(), angle.sin()];
            let a = mul([m[0].into(), m[1].into()], p);
            let b = mul([other[0].into(), -f64::from(other[1])], [p[0], -p[1]]);
            let h = [a[0] + b[0], a[1] + b[1]];
            frequency[i] = h;
            let minimum = crate::ocean::parameters::shore::ShoreConfig::default().filter_cutoff;
            let wavelength = TAU / k.max(1e-6);
            let t = ((wavelength - f64::from(minimum))
                / f64::from(
                    crate::ocean::parameters::shore::ShoreConfig::default().filter_transition,
                ))
            .clamp(0.0, 1.0);
            let filter = t * t * (3.0 - 2.0 * t);
            frequency[3 * count + i] = [h[0] * filter, h[1] * filter];
            if k > 0.0 {
                let factor = (9.81 * k).sqrt() / (k * k) * filter;
                frequency[4 * count + i] = [
                    (b[0] - a[0]) * factor * f64::from(m[2]),
                    (b[1] - a[1]) * factor * f64::from(m[2]),
                ];
                frequency[5 * count + i] = [
                    (b[0] - a[0]) * factor * f64::from(m[3]),
                    (b[1] - a[1]) * factor * f64::from(m[3]),
                ];
            }

            if k > 0.0 {
                frequency[count + i] = [h[1] * f64::from(m[2]) / k, -h[0] * f64::from(m[2]) / k];
                frequency[2 * count + i] =
                    [h[1] * f64::from(m[3]) / k, -h[0] * f64::from(m[3]) / k];
            }
        }
    }
    let mut result = vec![[0.0; 2]; 6 * count];
    for channel in 0..6 {
        for z in 0..n {
            for x in 0..n {
                let mut sum = [0.0; 2];
                for kz in 0..n {
                    for kx in 0..n {
                        let angle = TAU * (kx * x + kz * z) as f64 / n as f64;
                        let term = mul(
                            frequency[channel * count + kz * n + kx],
                            [angle.cos(), angle.sin()],
                        );
                        sum[0] += term[0];
                        sum[1] += term[1];
                    }
                }
                result[channel * count + z * n + x] =
                    [sum[0] / count as f64, sum[1] / count as f64];
            }
        }
    }
    result
}

// Single traveling modes give an independent signed momentum reference.
pub(in crate::ocean) fn verify_coupling(gpu: &Gpu) -> Result<(), Error> {
    let config = OceanConfig {
        resolution: 32,
        length: 128.0,
        ..Default::default()
    };
    let n = config.resolution as usize;
    let count = n * n;
    for (kx, kz) in [(1_i32, 1_i32), (-1, -1), (12, 0)] {
        let mut modes = initial_spectrum(config);
        for m in &mut modes {
            m[0] = 0.0;
            m[1] = 0.0;
        }
        let index = kz.rem_euclid(n as i32) as usize * n + kx.rem_euclid(n as i32) as usize;
        modes[index][0] = 0.2 * count as f32 / 2.0;
        let ocean = Ocean::from_spectrum(gpu, config, modes)?;
        for time in [0.0_f32, 1.3] {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            ocean.encode(gpu, &mut encoder, time);
            gpu.queue.submit([encoder.finish()]);
            let fields = ocean.read_fields(gpu)?;
            let k = TAU / config.length as f64 * ((kx * kx + kz * kz) as f64).sqrt();
            let omega = (9.81 * k).sqrt();
            for (x, z) in [(0, 0), (3, 7), (19, 21)] {
                let i = z * n + x;
                let phase = TAU * (kx as f64 * x as f64 + kz as f64 * z as f64) / n as f64
                    + omega * time as f64;
                let eta = 0.2 * phase.cos();
                let expected = if kx == 12 {
                    [0.0; 3]
                } else {
                    [
                        eta,
                        -omega / k * kx as f64 / ((kx * kx + kz * kz) as f64).sqrt() * eta,
                        -omega / k * kz as f64 / ((kx * kx + kz * kz) as f64).sqrt() * eta,
                    ]
                };
                for c in 0..3 {
                    if (fields[(3 + c) * count + i][0] as f64 - expected[c]).abs() > 0.00003 {
                        return Err(format!(
                            "FFT coupling signed-mode mismatch: k=({kx},{kz}), channel {c}"
                        )
                        .into());
                    }
                }
            }
        }
    }
    // Compare filter modes at identical time: far-ocean channels must stay identical.
    let ocean = Ocean::new(gpu, config)?;
    let mut unfiltered = Vec::new();
    for (index, enabled, cutoff, width) in [
        (0, false, 12.0, 12.0),
        (1, true, 0.0, 0.0),
        (2, true, 512.0, 0.0),
        (3, true, 3.0, 6.0),
    ] {
        let shore = crate::ocean::parameters::shore::ShoreConfig {
            filter_enabled: enabled,
            filter_cutoff: cutoff,
            filter_transition: width,
            ..Default::default()
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        ocean.encode_filtered(gpu, &mut encoder, 1.25, &shore);
        gpu.queue.submit([encoder.finish()]);
        let actual = ocean.read_fields(gpu)?;
        if index == 0 {
            unfiltered = actual.clone();
        }
        if actual[..3 * count] != unfiltered[..3 * count] {
            return Err("SWE filter changed FFT render channels".into());
        }
        if index <= 1
            && actual[3 * count..4 * count]
                .iter()
                .zip(&actual[..count])
                .any(|(a, b)| (a[0] - b[0]).abs() > 1e-5)
        {
            return Err("disabled/zero cutoff lost inlet elevation".into());
        }
        if index == 1 && actual != unfiltered {
            return Err("zero cutoff does not pass all modes".into());
        }
        if index == 2 && actual[3 * count..].iter().flatten().any(|v| v.abs() > 1e-6) {
            return Err("hard cutoff did not reject inlet modes".into());
        }
        if actual.iter().flatten().any(|v| !v.is_finite()) {
            return Err("invalid configurable filter output".into());
        }
    }
    println!(
        "FFT coupling: analytic oblique modes in both directions, phase and shared short-wave rejection OK"
    );
    Ok(())
}

pub(in crate::ocean) fn verify_fft(gpu: &Gpu) -> Result<(), Error> {
    for resolution in [8, 16] {
        let config = OceanConfig {
            resolution,
            length: 64.0,
            ..OceanConfig::default()
        };
        let ocean = Ocean::new(gpu, config)?;
        let mut first = Vec::new();
        for time in [0.0, 0.75, 3.5] {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            ocean.encode(gpu, &mut encoder, time);
            gpu.queue.submit([encoder.finish()]);
            let actual = ocean.read_fields(gpu)?;
            let expected = reference(config, time);
            let mut max_error = 0.0_f64;
            let mut max_imaginary = 0.0_f32;
            for (a, b) in actual.iter().zip(&expected) {
                for component in 0..2 {
                    if !a[component].is_finite() {
                        return Err("non-finite GPU result".into());
                    }
                    max_error = max_error.max((f64::from(a[component]) - b[component]).abs());
                }
                max_imaginary = max_imaginary.max(a[1].abs());
            }
            let count = config.resolution.pow(2) as usize;
            let mean = actual[..count].iter().map(|v| v[0]).sum::<f32>() / count as f32;
            if max_error > 0.00005 || max_imaginary > 0.00001 || mean.abs() > 0.00001 {
                return Err(format!(
                "GPU verification failed: error={max_error}, imaginary={max_imaginary}, mean={mean}"
            )
            .into());
            }
            if time == 0.0 {
                first = actual.clone();
            } else if actual[..count]
                .iter()
                .zip(&first)
                .all(|(a, b)| (a[0] - b[0]).abs() < 0.0001)
            {
                return Err("waves did not animate".into());
            }
            println!(
                "N={resolution}, t={time:.2}s: height/Dx/Dz/filtered-height/Qx/Qz max error={max_error:.3e}, imaginary={max_imaginary:.3e}, height mean={mean:.3e} OK"
            );
        }
    }
    println!("GPU IFFT agrees with direct f64 DFT at all three times.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_has_real_output_zero_mean_and_time_evolution() {
        let config = OceanConfig {
            resolution: 8,
            ..OceanConfig::default()
        };
        let a = reference(config, 0.0);
        let b = reference(config, 1.0);
        assert!(a.iter().all(|v| v[1].abs() < 1e-10));
        assert!(a[..64].iter().map(|v| v[0]).sum::<f64>().abs() < 1e-10);
        assert!(a.iter().zip(b).any(|(x, y)| (x[0] - y[0]).abs() > 0.01));
    }
}
