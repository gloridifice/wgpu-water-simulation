use std::f32::consts::TAU;

pub use crate::ocean::parameters::ocean::OceanConfig;

/// GPU 布局：h0 的复数实部、虚部，以及波向量 kx、kz。
pub fn initial_spectrum(config: OceanConfig) -> Vec<[f32; 4]> {
    let n = config.resolution as usize;
    let mut random = Gaussian::new(config.seed);
    let wind = [config.wind_angle.cos(), config.wind_angle.sin()];
    let largest_wave = config.phillips_length;
    let mut modes = vec![[0.0; 4]; n * n];
    for z in 0..n {
        for x in 0..n {
            let frequency = |i: usize| {
                let signed = if i < n / 2 {
                    i as i32
                } else {
                    i as i32 - n as i32
                };
                TAU * signed as f32 / config.length
            };
            let kx = frequency(x);
            let kz = frequency(z);
            let k2 = kx * kx + kz * kz;
            modes[z * n + x] = [0.0, 0.0, kx, kz];
            // 清除 DC 和 Nyquist 行列；避免偶数网格自共轭频率的方向歧义。
            if k2 == 0.0 || x == n / 2 || z == n / 2 {
                continue;
            }
            let alignment = (kx * wind[0] + kz * wind[1]) / k2.sqrt();
            let directional = alignment.powi(2) * if alignment < 0.0 { 1.0 } else { 0.07 };
            let phillips = config.amplitude
                * (-1.0 / (k2 * largest_wave.powi(2))).exp()
                * (-k2 * 0.12_f32.powi(2)).exp()
                * directional
                / k2.powi(2);
            let scale = (0.5 * phillips).sqrt();
            let re = random.sample() * scale;
            let im = random.sample() * scale;
            modes[z * n + x][0] = re;
            modes[z * n + x][1] = im;
        }
    }
    // 连续二维谱离散化：sqrt(dkx*dkz)=2pi/length。
    // IFFT 两轴除以 N，因此乘 N²；不再归一化波高，否则 A 会被抵消。
    let scale = (n * n) as f32 * TAU / config.length;
    for mode in &mut modes {
        mode[0] *= scale;
        mode[1] *= scale;
    }
    modes
}

struct Gaussian(u64);

impl Gaussian {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    fn uniform(&mut self) -> f32 {
        // SplitMix64：固定种子便于复现；不会每帧重新生成噪声。
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        ((z >> 40) as f32 + 0.5) / 16_777_216.0
    }

    fn sample(&mut self) -> f32 {
        (-2.0 * self.uniform().ln()).sqrt() * (TAU * self.uniform()).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectrum_is_reproducible_finite_and_has_no_dc_or_nyquist() {
        let config = OceanConfig::default();
        let a = initial_spectrum(config);
        assert_eq!(a, initial_spectrum(config));
        assert_ne!(a, initial_spectrum(OceanConfig { seed: 43, ..config }));
        let n = config.resolution as usize;
        assert!(a.iter().flatten().all(|x| x.is_finite()));
        assert_eq!(&a[0][..2], &[0.0, 0.0]);
        for i in 0..n {
            assert_eq!(&a[i * n + n / 2][..2], &[0.0, 0.0]);
            assert_eq!(&a[n / 2 * n + i][..2], &[0.0, 0.0]);
        }
    }

    #[test]
    fn invalid_configuration_is_rejected() {
        assert!(
            OceanConfig {
                resolution: 127,
                ..OceanConfig::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            OceanConfig {
                length: f32::NAN,
                ..OceanConfig::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            OceanConfig {
                amplitude: -1.0,
                ..OceanConfig::default()
            }
            .validate()
            .is_err()
        );
    }
}

#[cfg(test)]
mod amplitude_tests {
    use super::*;
    #[test]
    fn amplitude_scales_power_and_zero_flattens_water() {
        let config = OceanConfig::default();
        let a = initial_spectrum(config);
        let b = initial_spectrum(OceanConfig {
            amplitude: config.amplitude * 4.0,
            ..config
        });
        for (a, b) in a.iter().zip(b) {
            assert!((b[0] - 2.0 * a[0]).abs() < 1e-5);
            assert!((b[1] - 2.0 * a[1]).abs() < 1e-5);
        }
        assert!(
            initial_spectrum(OceanConfig {
                amplitude: 0.0,
                ..config
            })
            .iter()
            .all(|v| v[0] == 0.0 && v[1] == 0.0)
        );
    }
    #[test]
    fn mesh_settings_do_not_change_spectrum() {
        let config = OceanConfig::default();
        let reference = initial_spectrum(config);
        for (mesh_resolution, triangular_mesh) in [(47, false), (128, true), (47, true)] {
            assert_eq!(
                reference,
                initial_spectrum(OceanConfig {
                    mesh_resolution,
                    triangular_mesh,
                    ..config
                })
            );
        }
    }
}
