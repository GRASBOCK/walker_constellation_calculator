use std::f32::consts::PI;

/// Maximum geometrically valid sensor half-angle `α_max` [rad], given a planet
/// radius `R` and orbital altitude `h` (both in the same length unit).
///
/// Derived from the law-of-sines relation cos(ε) = R/(R+h)·sin(α). The
/// line of sight is tangent to the planet's limb when ε = 0, giving
/// `sin(α_max)` = R/(R+h).
///
/// (Note: the typst journal states arcsin((R+h)/R), but that argument is
/// always > 1; the correct horizon-tangency bound is arcsin(R/(R+h)).)
pub fn max_half_fov(radius: f32, altitude: f32) -> f32 {
    (radius / (radius + altitude)).asin()
}

/// Maximum geometrically valid full field of view `2·α_max` [rad].
///
/// `radius` and `altitude` must be in the same length unit.
pub fn max_fov(radius: f32, altitude: f32) -> f32 {
    2.0 * max_half_fov(radius, altitude)
}

/// A Walker Delta constellation, with all quantities in SI units (radians, meters, seconds).
pub struct Constellation {
    /// Orbital inclination [rad]
    pub inclination: f32,
    /// Number of satellites per plane (S)
    pub satellites: u32,
    /// Number of orbital planes (P)
    pub planes: u32,
    /// Altitude above surface [m]
    pub altitude: f32,
    /// Planet radius [m]
    pub radius: f32,
    /// Standard gravitational parameter [m³/s²]
    pub mu: f32,
    /// Planet angular rotation rate [rad/s]
    pub omega: f32,
    /// Sensor full field of view (2·α) [rad]
    pub fov: f32,
}

impl Constellation {
    /// Earth-central coverage half-angle σ of a single satellite footprint, in radians.
    ///
    /// σ = π/2 − α − arccos( R/(R+h) · sin(α) )
    ///
    /// Callers are expected to ensure `fov ≤ max_fov(radius, altitude)`;
    /// the arccos argument is clamped defensively.
    pub fn coverage_half_angle(&self) -> f32 {
        let alpha = self.fov * 0.5;
        let ratio = (self.radius + self.altitude) / self.radius;
        let x = (ratio * alpha.sin()).clamp(-1.0, 1.0);
        x.asin() - alpha
    }

    /// Effective equatorial swath `λ_swath` = 2σ / sin(i), in radians.
    pub fn effective_swath(&self) -> Option<f32> {
        let sin_i = self.inclination.sin();
        if sin_i.abs() < f32::EPSILON {
            return None;
        }
        Some(2.0 * self.coverage_half_angle() / sin_i)
    }

    /// Orbital period in seconds, `T_orb` = 2π · sqrt(a³/μ), with a = R + h.
    pub fn orbital_period(&self) -> f32 {
        let a = self.radius + self.altitude;
        2.0 * PI * (a.powi(3) / self.mu).sqrt()
    }

    /// Maximum revisit time at the equator, in seconds.
    ///
    /// Implements the regimes from `walker_delta.typ`:
    /// - Regime 1 (`λ_gap` ≤ 0): swaths overlap, `t_rev` = `T_orb` / S
    /// - Regime 2 (`λ_gap` > 0): `t_rev` = `λ_gap` / ω + `T_orb` / S
    ///
    /// Returns `None` if the geometry is invalid (e.g. FOV too large,
    /// zero satellites/planes, or non-overlapping swaths around a non-rotating planet).
    pub fn max_revisit_time(&self) -> Option<f32> {
        if self.satellites == 0 || self.planes == 0 {
            return None;
        }
        let swath = self.effective_swath()?;
        let delta_omega = 2.0 * PI / self.planes as f32;
        let lambda_gap = delta_omega - swath;

        let t_orb = self.orbital_period();
        let in_plane = t_orb / self.satellites as f32;

        if lambda_gap <= 0.0 {
            Some(in_plane)
        } else {
            if self.omega.abs() < f32::EPSILON {
                return None;
            }
            Some(lambda_gap / self.omega.abs() + in_plane)
        }
    }
}
