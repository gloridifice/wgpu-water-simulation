//! Static bottom-attached obstacles shared by SWE and opaque rendering.
pub const MAX_COLLIDERS: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    #[default]
    Cylinder,
    Box,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Collider {
    pub enabled: bool,
    pub shape: Shape,
    pub position: [f32; 2],
    pub radius: f32,
    pub size: [f32; 2],
    pub height: f32,
    pub angle: f32,
}
impl Default for Collider {
    fn default() -> Self {
        Self {
            enabled: true,
            shape: Shape::Cylinder,
            position: [0.0; 2],
            radius: 2.0,
            size: [4.0, 8.0],
            height: 4.0,
            angle: 0.0,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ColliderUniform {
    pub footprint: [f32; 4], // center X/Z, radius or half extent X/Z
    pub solid: [f32; 4],     // 1 cylinder / 2 box, absolute top Y, cos/sin angle
}
impl Collider {
    pub fn validate(self) -> Result<(), String> {
        for (value, low, high) in [
            (self.position[0], -1000.0, 1000.0),
            (self.position[1], -1000.0, 1000.0),
            (self.radius, 0.25, 30.0),
            (self.size[0], 0.5, 60.0),
            (self.size[1], 0.5, 60.0),
            (self.height, 0.1, 30.0),
            (self.angle, -180.0, 180.0),
        ] {
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err("invalid collider position, size, height or angle".into());
            }
        }
        Ok(())
    }
    pub fn extent(self) -> [f32; 2] {
        if self.shape == Shape::Cylinder {
            return [self.radius; 2];
        }
        let (sin, cos) = self.angle.to_radians().sin_cos();
        [
            0.5 * (cos.abs() * self.size[0] + sin.abs() * self.size[1]),
            0.5 * (sin.abs() * self.size[0] + cos.abs() * self.size[1]),
        ]
    }
    /// Keep whole footprints out of the FFT relaxation boundary and off-grid.
    pub fn in_domain(self, domain: [f32; 4], resolution: u32) -> bool {
        let extent = self.extent();
        let pad = [domain[2] / resolution as f32, domain[3] / resolution as f32];
        (0..2).all(|a| {
            self.position[a] - extent[a] >= domain[a] + pad[a]
                && self.position[a] + extent[a]
                    <= domain[a] + domain[a + 2] * if a == 0 { 0.78 } else { 1.0 } - pad[a]
        })
    }
    pub fn uniform(self, slope: f32, shore_x: f32) -> ColliderUniform {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let half = if self.shape == Shape::Cylinder {
            [self.radius; 2]
        } else {
            [self.size[0] * 0.5, self.size[1] * 0.5]
        };
        ColliderUniform {
            footprint: [self.position[0], self.position[1], half[0], half[1]],
            solid: [
                if self.shape == Shape::Cylinder {
                    1.0
                } else {
                    2.0
                },
                -slope * (self.position[0] - shore_x) + self.height,
                cos,
                sin,
            ],
        }
    }
}
pub fn packed(
    colliders: &[Option<Collider>; MAX_COLLIDERS],
    domain: [f32; 4],
    slope: f32,
    shore_x: f32,
    resolution: u32,
) -> ([ColliderUniform; MAX_COLLIDERS], u32) {
    let mut result = [bytemuck::Zeroable::zeroed(); MAX_COLLIDERS];
    let mut count = 0;
    for c in colliders
        .iter()
        .flatten()
        .filter(|c| c.enabled && c.in_domain(domain, resolution))
    {
        result[count] = c.uniform(slope, shore_x);
        count += 1;
    }
    (result, count as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_configs_and_obstacles_round_trip() {
        let mut config: crate::ocean::parameters::shore::ShoreConfig =
            serde_json::from_str("{}").unwrap();
        assert!(config.colliders.iter().all(Option::is_none));
        config.colliders[0] = Some(Collider::default());
        config.colliders[7] = Some(Collider {
            shape: Shape::Box,
            angle: 45.0,
            position: [8.0, -4.0],
            ..Default::default()
        });
        config.validate().unwrap();
        assert_eq!(
            config,
            serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap()
        );
    }
    #[test]
    fn rotated_bounds_and_packing_exclude_inactive_and_outside() {
        let domain = [-12.0, -32.0, 64.0, 64.0];
        let c = Collider {
            shape: Shape::Box,
            position: [8.0, 0.0],
            angle: 90.0,
            ..Default::default()
        };
        assert!((c.extent()[0] - 4.0).abs() < 1e-6 && (c.extent()[1] - 2.0).abs() < 1e-6);
        let mut list = [None; MAX_COLLIDERS];
        list[0] = Some(Collider {
            position: [100.0, 0.0],
            ..c
        });
        list[2] = Some(Collider {
            enabled: false,
            ..c
        });
        list[6] = Some(c);
        let (packed, count) = packed(&list, domain, 0.18, 0.0, 256);
        assert_eq!(count, 1);
        assert_eq!(packed[0], c.uniform(0.18, 0.0));
        assert_eq!(packed[1].solid, [0.0; 4]);
        assert!((packed[0].solid[1] - 2.56).abs() < 1e-6);
    }
    #[test]
    fn invalid_geometry_is_rejected_even_when_disabled() {
        for invalid in [
            Collider {
                position: [f32::NAN, 0.0],
                ..Default::default()
            },
            Collider {
                radius: 0.0,
                ..Default::default()
            },
            Collider {
                size: [-1.0, 4.0],
                ..Default::default()
            },
            Collider {
                angle: f32::INFINITY,
                ..Default::default()
            },
            Collider {
                height: 30.1,
                ..Default::default()
            },
        ] {
            assert!(
                Collider {
                    enabled: false,
                    ..invalid
                }
                .validate()
                .is_err()
            );
        }
    }
}
