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

pub struct SimulationInput {
    /// the timespan that the user can see [s]
    pub timespan: f32,
    /// how far into the future the prediction will run [s]
    pub max_predicition_time: f32,
    /// sample rate in seconds
    pub dt: f32,
}

pub struct SimulationData {
    /// the groundtrack as a vector of (lat, lon) coordinates for each satellite
    pub groundtrack: Vec<Vec<(f32, f32)>>,
    /// coverage edges as a vector of left and right (lat, lon) coordinates for each satellite
    pub coverage_edge: Vec<Vec<((f32, f32), (f32, f32))>>,
    /// time vector
    pub time: Vec<f32>,
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

    /// Simulate satellite ground tracks and instantaneous coverage edges
    /// in a planet-fixed (ECEF-like) frame, sampled at `inp.dt` over
    /// `[0, inp.max_predicition_time + inp.timespan]`.
    ///
    /// Assumptions:
    /// - Circular orbits at radius `a = R + h`, mean motion `n = √(μ/a³)`.
    /// - Walker Delta RAAN spread of 2π (Δω = 2π/P).
    /// - Phasing parameter F = 0 (hardcoded for now).
    /// - Epoch (t = 0): plane 0's RAAN = 0; satellite (p=0, s=0) at its
    ///   ascending node; in-plane offsets s·(2π/S); inter-plane phasing
    ///   p·F·2π/(S·P).
    /// - Output (lat, lon) is in radians in a planet-fixed frame: the
    ///   inertial position is rotated by −ω·t about z before projection.
    /// - Coverage edges are the two points on the planet's surface at
    ///   central angle σ from the sub-satellite point, perpendicular to
    ///   the ground-track velocity (left, right of motion).
    pub fn revisit_time_simulation(&self, inp: SimulationInput) -> SimulationData {
        let sigma = self.coverage_half_angle();
        let a = self.radius + self.altitude;
        let n = 2.0 * PI / self.orbital_period(); // mean motion [rad/s]
        let i = self.inclination;

        const F: u32 = 0; // Walker phasing parameter (hardcoded)

        let two_pi = 2.0 * PI;
        let total_sats = (self.satellites as usize) * (self.planes as usize);
        let total_time = inp.max_predicition_time + inp.timespan;
        let dt = inp.dt.max(f32::EPSILON);
        let n_steps = (total_time / dt).ceil() as usize + 1;

        let d_raan = two_pi / self.planes.max(1) as f32;
        let d_mean_in_plane = two_pi / self.satellites.max(1) as f32;
        let d_mean_between_planes = if self.satellites > 0 && self.planes > 0 {
            two_pi * F as f32 / (self.satellites * self.planes) as f32
        } else {
            0.0
        };

        let mut groundtrack: Vec<Vec<(f32, f32)>> = (0..total_sats)
            .map(|_| Vec::with_capacity(n_steps))
            .collect();
        let mut coverage_edge: Vec<Vec<((f32, f32), (f32, f32))>> = (0..total_sats)
            .map(|_| Vec::with_capacity(n_steps))
            .collect();
        let mut time = Vec::with_capacity(n_steps);

        for step in 0..n_steps {
            let t = step as f32 * dt;
            time.push(t);
            let theta = self.omega * t; // planet rotation angle at time t

            for p in 0..self.planes {
                let raan = p as f32 * d_raan;
                for s in 0..self.satellites {
                    let idx = (p * self.satellites + s) as usize;
                    let nu = s as f32 * d_mean_in_plane + p as f32 * d_mean_between_planes + n * t;

                    // perifocal frame (circular orbit, argument of periapsis = 0)
                    let (sn, cn) = nu.sin_cos();
                    let r_pf = [a * cn, a * sn, 0.0];
                    let v_pf = [-a * n * sn, a * n * cn, 0.0];

                    // rotate by inclination (about x), then RAAN (about z) -> ECI
                    let r_eci = rot_z(rot_x(r_pf, i), raan);
                    let v_eci = rot_z(rot_x(v_pf, i), raan);

                    // ECI -> ECEF: rotate by -theta about z, then subtract
                    // planet-rotation contribution from the velocity:
                    //   v_ecef = R_z(-θ) v_eci - ω × r_ecef
                    let r_ecef = rot_z(r_eci, -theta);
                    let v_rot = rot_z(v_eci, -theta);
                    let v_ecef = [
                        v_rot[0] - (-self.omega * r_ecef[1]),
                        v_rot[1] - (self.omega * r_ecef[0]),
                        v_rot[2],
                    ];

                    // sub-satellite unit vector
                    let r_hat = normalize(r_ecef);

                    // ground-track tangent: project v into the local tangent plane
                    let vr = dot(v_ecef, r_hat);
                    let v_tan = [
                        v_ecef[0] - vr * r_hat[0],
                        v_ecef[1] - vr * r_hat[1],
                        v_ecef[2] - vr * r_hat[2],
                    ];
                    let t_hat = normalize(v_tan);

                    // cross-track unit vector (in tangent plane, right-hand of motion)
                    let c_hat = normalize(cross(t_hat, r_hat));

                    // (lat, lon) of sub-satellite point
                    let sub_ll = to_lat_lon(r_hat);
                    groundtrack[idx].push(sub_ll);

                    // edge points on the unit sphere at central angle σ along ±c_hat
                    let (ss, cs) = sigma.sin_cos();
                    let right = [
                        cs * r_hat[0] + ss * c_hat[0],
                        cs * r_hat[1] + ss * c_hat[1],
                        cs * r_hat[2] + ss * c_hat[2],
                    ];
                    let left = [
                        cs * r_hat[0] - ss * c_hat[0],
                        cs * r_hat[1] - ss * c_hat[1],
                        cs * r_hat[2] - ss * c_hat[2],
                    ];
                    coverage_edge[idx].push((to_lat_lon(left), to_lat_lon(right)));
                }
            }
        }

        SimulationData {
            groundtrack,
            coverage_edge,
            time,
        }
    }
}

// --- small vector helpers (3D, f32) -----------------------------------------

fn rot_x(v: [f32; 3], a: f32) -> [f32; 3] {
    let (s, c) = a.sin_cos();
    [v[0], c * v[1] - s * v[2], s * v[1] + c * v[2]]
}

fn rot_z(v: [f32; 3], a: f32) -> [f32; 3] {
    let (s, c) = a.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1], v[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let m = dot(v, v).sqrt();
    if m < f32::EPSILON {
        [0.0, 0.0, 0.0]
    } else {
        [v[0] / m, v[1] / m, v[2] / m]
    }
}

/// Convert a unit vector (in a planet-fixed frame) to (lat, lon) in radians.
fn to_lat_lon(u: [f32; 3]) -> (f32, f32) {
    let lat = u[2].clamp(-1.0, 1.0).asin();
    let lon = u[1].atan2(u[0]);
    (lat, lon)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Rough Earth-ish parameters for smoke tests.
    fn earth_like(inclination_deg: f32) -> Constellation {
        Constellation {
            inclination: inclination_deg.to_radians(),
            satellites: 4,
            planes: 3,
            altitude: 550_000.0,        // 550 km
            radius: 6_371_000.0,        // 6371 km
            mu: 3.986_004_418e14,       // m^3/s^2
            omega: 7.292_115e-5,        // rad/s
            fov: 60.0_f32.to_radians(), // 60° full FoV
        }
    }

    #[test]
    fn simulation_smoke_basic_shape_and_bounds() {
        let c = earth_like(53.0);
        let inp = SimulationInput {
            timespan: 0.0,
            max_predicition_time: c.orbital_period(),
            dt: 60.0, // 1-minute samples
        };
        let data = c.revisit_time_simulation(inp);

        let total_sats = (c.satellites * c.planes) as usize;
        assert_eq!(data.groundtrack.len(), total_sats);
        assert_eq!(data.coverage_edge.len(), total_sats);
        assert!(!data.time.is_empty());
        for track in &data.groundtrack {
            assert_eq!(track.len(), data.time.len());
        }
        for edges in &data.coverage_edge {
            assert_eq!(edges.len(), data.time.len());
        }

        // Latitude must stay within ±i (with a small numerical slack).
        let i = c.inclination;
        for track in &data.groundtrack {
            for (lat, lon) in track {
                assert!(lat.abs() <= i + 1e-3, "|lat|={} > i={}", lat, i);
                assert!(lon.is_finite());
                assert!(lon.abs() <= PI + 1e-5);
            }
        }
    }

    #[test]
    fn simulation_equatorial_orbit_stays_on_equator() {
        let c = Constellation {
            inclination: 0.0,
            ..earth_like(0.0)
        };
        let inp = SimulationInput {
            timespan: 0.0,
            max_predicition_time: c.orbital_period(),
            dt: 30.0,
        };
        // Note: inclination = 0 makes effective_swath() undefined, but the
        // simulation itself doesn't depend on it.
        let data = c.revisit_time_simulation(inp);
        for track in &data.groundtrack {
            for (lat, _lon) in track {
                assert!(
                    lat.abs() < 1e-4,
                    "equatorial track left equator: lat={}",
                    lat
                );
            }
        }
    }

    #[test]
    fn coverage_edges_are_at_central_angle_sigma() {
        let c = earth_like(53.0);
        let sigma = c.coverage_half_angle();
        let inp = SimulationInput {
            timespan: 0.0,
            max_predicition_time: c.orbital_period() * 0.25,
            dt: 120.0,
        };
        let data = c.revisit_time_simulation(inp);

        // For each sample, the angular distance from sub-sat to each edge
        // (computed on the unit sphere) should equal σ.
        for (sat_track, sat_edges) in data.groundtrack.iter().zip(&data.coverage_edge) {
            for ((lat, lon), (left, right)) in sat_track.iter().zip(sat_edges) {
                let sub = ll_to_unit(*lat, *lon);
                let l = ll_to_unit(left.0, left.1);
                let r = ll_to_unit(right.0, right.1);
                let d_left = dot(sub, l).clamp(-1.0, 1.0).acos();
                let d_right = dot(sub, r).clamp(-1.0, 1.0).acos();
                assert!(
                    (d_left - sigma).abs() < 1e-3,
                    "left edge angle {} vs σ {}",
                    d_left,
                    sigma
                );
                assert!(
                    (d_right - sigma).abs() < 1e-3,
                    "right edge angle {} vs σ {}",
                    d_right,
                    sigma
                );
            }
        }
    }

    fn ll_to_unit(lat: f32, lon: f32) -> [f32; 3] {
        let (slat, clat) = lat.sin_cos();
        let (slon, clon) = lon.sin_cos();
        [clat * clon, clat * slon, slat]
    }
}
