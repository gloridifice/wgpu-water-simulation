//! 供渲染器或 CPU 验证使用的水体光学数学。
//!
//! 实现无偏振介电质 Fresnel、Snell 折射和均匀介质 Beer–Lambert 吸收。
//! 光程由调用者根据场景求交提供，单位为米，不能用模拟的光学层厚度代替。
//! 所有颜色均为线性 RGB 辐亮度；这里不做光线追踪、体散射、色调映射或伽马编码。

use std::error::Error;
use std::fmt;

use crate::grid::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpticsError {
    InvalidRefractiveIndex,
    InvalidCosine,
    InvalidDirection,
    NormalFacesAway,
    InvalidAbsorption,
    InvalidDistance,
    InvalidRadiance,
    InvalidWeight,
}

impl fmt::Display for OpticsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidRefractiveIndex => "折射率必须是有限正数",
            Self::InvalidCosine => "入射余弦必须是 0 到 1 的有限数",
            Self::InvalidDirection => "光线和法线必须是有限非零向量",
            Self::NormalFacesAway => "法线必须朝向入射介质，与入射方向相对",
            Self::InvalidAbsorption => "吸收系数必须是有限非负数，单位为每米",
            Self::InvalidDistance => "光程必须是有限非负数，单位为米",
            Self::InvalidRadiance => "线性 RGB 辐亮度必须是有限非负数",
            Self::InvalidWeight => "透射率和反射率必须是 0 到 1 的有限数",
        };
        f.write_str(message)
    }
}

impl Error for OpticsError {}

/// 计算无偏振光的精确 Fresnel 能量反射率。
///
/// `cos_incident` 是入射方向与朝向入射侧法线之间夹角的绝对余弦。
/// `eta_incident`、`eta_transmitted` 分别为光线进入界面前后的折射率。
/// 全反射返回 `1`；两侧折射率相同时返回 `0`。
pub fn fresnel_dielectric(
    cos_incident: f64,
    eta_incident: f64,
    eta_transmitted: f64,
) -> Result<f64, OpticsError> {
    check_indices(eta_incident, eta_transmitted)?;
    if !cos_incident.is_finite() || !(0.0..=1.0).contains(&cos_incident) {
        return Err(OpticsError::InvalidCosine);
    }
    if eta_incident == eta_transmitted {
        return Ok(0.0);
    }
    if cos_incident == 0.0 {
        return Ok(1.0);
    }
    let sin_incident = (1.0 - cos_incident * cos_incident).max(0.0).sqrt();
    let sin_transmitted = transmitted_sine(sin_incident, eta_incident, eta_transmitted);
    if sin_transmitted >= 1.0 {
        return Ok(1.0);
    }
    let cos_transmitted = (1.0 - sin_transmitted * sin_transmitted).sqrt();
    // 同比例缩放折射率不会改变 Fresnel，且可避免 n*cos 项溢出。
    let scale = eta_incident.max(eta_transmitted);
    let ni = eta_incident / scale;
    let nt = eta_transmitted / scale;
    let perpendicular =
        (ni * cos_incident - nt * cos_transmitted) / (ni * cos_incident + nt * cos_transmitted);
    let parallel =
        (nt * cos_incident - ni * cos_transmitted) / (nt * cos_incident + ni * cos_transmitted);
    Ok((0.5 * (perpendicular * perpendicular + parallel * parallel)).clamp(0.0, 1.0))
}

/// 返回镜面反射方向。输入可不归一化，输出为单位向量。
///
/// `incident` 沿光传播方向指向表面，`normal` 是表面法线。
pub fn reflect(incident: Vec3, normal: Vec3) -> Result<Vec3, OpticsError> {
    let incident = unit(incident)?;
    let normal = unit(normal)?;
    let projected = dot(incident, normal);
    unit(std::array::from_fn(|axis| {
        incident[axis] - 2.0 * projected * normal[axis]
    }))
}

/// 返回 Snell 定律给出的透射单位方向；全反射或临界掠射返回 `None`。
///
/// 入射方向沿光传播方向指向表面，法线必须朝向入射介质。离开水时调用者
/// 要翻转水体外法线，并交换两侧折射率。方向向量可以不预先归一化。
pub fn refract(
    incident: Vec3,
    normal: Vec3,
    eta_incident: f64,
    eta_transmitted: f64,
) -> Result<Option<Vec3>, OpticsError> {
    check_indices(eta_incident, eta_transmitted)?;
    let incident = unit(incident)?;
    let normal = unit(normal)?;
    let alignment = dot(incident, normal);
    if alignment > 1e-12 {
        return Err(OpticsError::NormalFacesAway);
    }
    let cos_incident = (-alignment).clamp(0.0, 1.0);
    if eta_incident == eta_transmitted {
        return Ok(Some(incident));
    }
    let sin_incident = (1.0 - cos_incident * cos_incident).max(0.0).sqrt();
    let sin_transmitted = transmitted_sine(sin_incident, eta_incident, eta_transmitted);
    if sin_transmitted >= 1.0 {
        return Ok(None);
    }
    let cos_transmitted = (1.0 - sin_transmitted * sin_transmitted).sqrt();
    if sin_transmitted == 0.0 {
        return Ok(Some(normal.map(|x| -x)));
    }
    let tangent = unit(std::array::from_fn(|axis| {
        incident[axis] + cos_incident * normal[axis]
    }))?;
    let transmitted = std::array::from_fn(|axis| {
        tangent[axis] * sin_transmitted - normal[axis] * cos_transmitted
    });
    Ok(Some(unit(transmitted)?))
}

/// 均匀介质沿给定光程的 RGB 吸收透射率 `exp(-sigma_a * length)`。
///
/// `absorption_per_meter` 为每米吸收系数，`path_length_meters` 为水中光程。
/// 本函数不包含散射；吸收系数为零的通道始终返回 `1`。
pub fn beer_lambert(
    absorption_per_meter: Vec3,
    path_length_meters: f64,
) -> Result<Vec3, OpticsError> {
    if absorption_per_meter
        .iter()
        .any(|x| !x.is_finite() || *x < 0.0)
    {
        return Err(OpticsError::InvalidAbsorption);
    }
    if !path_length_meters.is_finite() || path_length_meters < 0.0 {
        return Err(OpticsError::InvalidDistance);
    }
    Ok(absorption_per_meter.map(|sigma| (-sigma * path_length_meters).exp()))
}

/// 混合已采样的反射、折射光：`F*reflection + (1-F)*T*transmission`。
///
/// 这是供实时着色器验证的单界面吸收混合，不是完整的光传输积分器；
/// 不含介质散射、多次反射及跨介质辐亮度的折射率平方换算。调用者负责
/// 沿反射/折射方向取得相应辐亮度，并求出真实水中光程。输入允许 HDR。
pub fn mix_interface_radiance(
    reflected_radiance: Vec3,
    transmitted_radiance: Vec3,
    transmittance: Vec3,
    fresnel: f64,
) -> Result<Vec3, OpticsError> {
    if reflected_radiance
        .iter()
        .chain(&transmitted_radiance)
        .any(|x| !x.is_finite() || *x < 0.0)
    {
        return Err(OpticsError::InvalidRadiance);
    }
    if !fresnel.is_finite()
        || !(0.0..=1.0).contains(&fresnel)
        || transmittance
            .iter()
            .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
    {
        return Err(OpticsError::InvalidWeight);
    }
    Ok(std::array::from_fn(|axis| {
        fresnel * reflected_radiance[axis]
            + (1.0 - fresnel) * transmittance[axis] * transmitted_radiance[axis]
    }))
}

fn check_indices(incident: f64, transmitted: f64) -> Result<(), OpticsError> {
    if incident.is_finite() && transmitted.is_finite() && incident > 0.0 && transmitted > 0.0 {
        Ok(())
    } else {
        Err(OpticsError::InvalidRefractiveIndex)
    }
}

fn transmitted_sine(sin_incident: f64, ni: f64, nt: f64) -> f64 {
    if sin_incident == 0.0 {
        // 避免极大折射率比值乘零形成 NaN。
        0.0
    } else if ni > nt {
        // 先乘不大于 1 的正弦，防止比值先溢出后丢掉可表示的结果。
        (ni * sin_incident) / nt
    } else {
        (ni / nt) * sin_incident
    }
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn unit(v: Vec3) -> Result<Vec3, OpticsError> {
    if v.iter().any(|x| !x.is_finite()) {
        return Err(OpticsError::InvalidDirection);
    }
    let scale = v.iter().fold(0.0_f64, |max, x| max.max(x.abs()));
    if scale == 0.0 {
        return Err(OpticsError::InvalidDirection);
    }
    let scaled = v.map(|x| x / scale);
    let length = dot(scaled, scaled).sqrt();
    Ok(scaled.map(|x| x / length))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresnel_agrees_with_normal_incidence_and_grazing_limits() {
        let expected = ((1.0_f64 - 1.333) / (1.0 + 1.333)).powi(2);
        assert!((fresnel_dielectric(1.0, 1.0, 1.333).unwrap() - expected).abs() < 1e-14);
        assert_eq!(fresnel_dielectric(0.0, 1.0, 1.333).unwrap(), 1.0);
        for cosine in [0.0, 0.1, 0.5, 1.0] {
            assert_eq!(fresnel_dielectric(cosine, 1.333, 1.333).unwrap(), 0.0);
        }
        assert!(fresnel_dielectric(0.1, 1.0, 1.333).unwrap() > expected);
    }

    #[test]
    fn snell_refraction_obeys_angle_ratio_and_is_reversible() {
        let incident = [0.6, -0.8, 0.0];
        let transmitted = refract(incident, [0.0, 1.0, 0.0], 1.0, 1.333)
            .unwrap()
            .unwrap();
        assert!((transmitted[0] - 0.6 / 1.333).abs() < 1e-14);
        assert!(transmitted[1] < 0.0);
        assert!((dot(transmitted, transmitted) - 1.0).abs() < 1e-14);
        let back = refract(transmitted.map(|x| -x), [0.0, -1.0, 0.0], 1.333, 1.0)
            .unwrap()
            .unwrap();
        for axis in 0..3 {
            assert!((back[axis] + incident[axis]).abs() < 1e-14);
        }
    }

    #[test]
    fn total_internal_reflection_has_no_transmitted_ray() {
        let incident = [3.0_f64.sqrt() * 0.5, -0.5, 0.0];
        assert_eq!(
            refract(incident, [0.0, 1.0, 0.0], 1.333, 1.0).unwrap(),
            None
        );
        assert_eq!(fresnel_dielectric(0.5, 1.333, 1.0).unwrap(), 1.0);
        let reflected = reflect(incident, [0.0, 4.0, 0.0]).unwrap();
        assert!((reflected[0] - incident[0]).abs() < 1e-14);
        assert!((reflected[1] - 0.5).abs() < 1e-14);
    }

    #[test]
    fn absorption_composes_over_successive_lengths_and_preserves_clear_channels() {
        let absorption = [0.0, 0.3, 0.8];
        let a = beer_lambert(absorption, 2.0).unwrap();
        let b = beer_lambert(absorption, 3.0).unwrap();
        let combined = beer_lambert(absorption, 5.0).unwrap();
        assert_eq!(a[0], 1.0);
        for axis in 0..3 {
            assert!((combined[axis] - a[axis] * b[axis]).abs() < 1e-14);
        }
        assert_eq!(beer_lambert(absorption, 0.0).unwrap(), [1.0; 3]);
        assert_eq!(beer_lambert([f64::MAX; 3], f64::MAX).unwrap(), [0.0; 3]);
    }

    #[test]
    fn radiance_mix_conserves_equal_inputs_without_absorption() {
        let light = [2.0, 1.0, 0.5];
        for f in [0.0, 0.2, 0.5, 1.0] {
            assert_eq!(
                mix_interface_radiance(light, light, [1.0; 3], f).unwrap(),
                light
            );
        }
        assert_eq!(
            mix_interface_radiance(light, light, [0.0; 3], 0.25).unwrap(),
            [0.5, 0.25, 0.125]
        );
    }

    #[test]
    fn rejects_invalid_optical_parameters() {
        assert_eq!(
            fresnel_dielectric(f64::NAN, 1.0, 1.333),
            Err(OpticsError::InvalidCosine)
        );
        assert_eq!(
            fresnel_dielectric(0.5, 0.0, 1.333),
            Err(OpticsError::InvalidRefractiveIndex)
        );
        assert_eq!(
            reflect([0.0; 3], [0.0, 1.0, 0.0]),
            Err(OpticsError::InvalidDirection)
        );
        assert_eq!(
            refract([0.0, 1.0, 0.0], [0.0, 1.0, 0.0], 1.0, 1.333),
            Err(OpticsError::NormalFacesAway)
        );
        assert_eq!(
            beer_lambert([-0.1, 0.0, 0.0], 1.0),
            Err(OpticsError::InvalidAbsorption)
        );
        assert_eq!(
            beer_lambert([0.1; 3], f64::INFINITY),
            Err(OpticsError::InvalidDistance)
        );
    }

    #[test]
    fn finite_extreme_indices_and_directions_remain_well_defined() {
        assert_eq!(
            fresnel_dielectric(1.0, f64::MAX, f64::MIN_POSITIVE).unwrap(),
            1.0
        );
        assert_eq!(
            fresnel_dielectric(0.0, f64::MIN_POSITIVE, f64::MAX).unwrap(),
            1.0
        );
        let direction = refract(
            [0.0, -f64::MAX, 0.0],
            [0.0, f64::MAX, 0.0],
            f64::MAX,
            f64::MIN_POSITIVE,
        )
        .unwrap()
        .unwrap();
        assert_eq!(direction, [0.0, -1.0, 0.0]);
    }
}
