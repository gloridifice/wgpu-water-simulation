//! Water optics mathematics for renderers and CPU verification.
//!
//! Implements unpolarized dielectric Fresnel, Snell refraction, and homogeneous
//! Beer–Lambert absorption. The caller supplies the optical path length in meters
//! from scene intersections; simulated optical layer thickness is not a substitute.
//! Colors are linear RGB radiance. No ray tracing, volume scattering, tone mapping,
//! or gamma encoding is performed here.

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
            Self::InvalidRefractiveIndex => "refractive indices must be finite and positive",
            Self::InvalidCosine => "incident cosine must be finite and between 0 and 1",
            Self::InvalidDirection => "ray and normal must be finite nonzero vectors",
            Self::NormalFacesAway => {
                "normal must face the incident medium, opposite the incident direction"
            }
            Self::InvalidAbsorption => {
                "absorption coefficients must be finite and nonnegative, in inverse meters"
            }
            Self::InvalidDistance => {
                "optical path length must be finite and nonnegative, in meters"
            }
            Self::InvalidRadiance => "linear RGB radiance must be finite and nonnegative",
            Self::InvalidWeight => {
                "transmittance and reflectance must be finite and between 0 and 1"
            }
        };
        f.write_str(message)
    }
}

impl Error for OpticsError {}

/// Calculate the exact Fresnel energy reflectance for unpolarized light.
///
/// `cos_incident` is the absolute cosine of the angle between the incident
/// direction and the normal facing the incident side. `eta_incident` and
/// `eta_transmitted` are the refractive indices before and after the interface.
/// Total internal reflection returns `1`; equal indices return `0`.
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
    // Uniformly scaling the indices preserves Fresnel and avoids n*cos overflow.
    let scale = eta_incident.max(eta_transmitted);
    let ni = eta_incident / scale;
    let nt = eta_transmitted / scale;
    let perpendicular =
        (ni * cos_incident - nt * cos_transmitted) / (ni * cos_incident + nt * cos_transmitted);
    let parallel =
        (nt * cos_incident - ni * cos_transmitted) / (nt * cos_incident + ni * cos_transmitted);
    Ok((0.5 * (perpendicular * perpendicular + parallel * parallel)).clamp(0.0, 1.0))
}

/// Return the specular reflection direction as a unit vector; inputs need not be normalized.
///
/// `incident` points toward the surface along light propagation; `normal` is the surface normal.
pub fn reflect(incident: Vec3, normal: Vec3) -> Result<Vec3, OpticsError> {
    let incident = unit(incident)?;
    let normal = unit(normal)?;
    let projected = dot(incident, normal);
    unit(std::array::from_fn(|axis| {
        incident[axis] - 2.0 * projected * normal[axis]
    }))
}

/// Return the unit transmission direction from Snell's law, or `None` for
/// total internal reflection or critical grazing incidence.
///
/// The incident direction points toward the surface along light propagation;
/// the normal must face the incident medium. When leaving water, the caller
/// must flip the outward normal and swap the indices. Inputs need not be normalized.
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

/// RGB absorption transmittance `exp(-sigma_a * length)` along a path in a homogeneous medium.
///
/// `absorption_per_meter` is the absorption coefficient per meter, and
/// `path_length_meters` is the optical path in water. Scattering is excluded;
/// channels with zero absorption always return `1`.
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

/// Mix sampled reflected and refracted light: `F*reflection + (1-F)*T*transmission`.
///
/// This single-interface absorption blend supports real-time shader validation;
/// it is not a full light transport integrator. It excludes medium scattering,
/// multiple reflections, and refractive-index-squared radiance conversion across
/// media. The caller supplies radiance along both directions and the actual path
/// length in water. HDR inputs are allowed.
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
        // Avoid NaN from multiplying an extreme index ratio by zero.
        0.0
    } else if ni > nt {
        // Multiply by the sine first to avoid overflowing the ratio before a representable result.
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
