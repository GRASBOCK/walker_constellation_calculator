use std::f64::consts::PI;

/// Maximum geometrically valid sensor half-angle `α_max` [rad], given a planet
/// radius `R` and orbital altitude `h` (both in the same length unit).
///
/// Derived from the law-of-sines relation cos(ε) = R/(R+h)·sin(α). The
/// line of sight is tangent to the planet's limb when ε = 0, giving
/// `sin(α_max)` = R/(R+h).
///
/// (Note: the typst journal states arcsin((R+h)/R), but that argument is
/// always > 1; the correct horizon-tangency bound is arcsin(R/(R+h)).)
pub fn max_half_fov(radius: f64, altitude: f64) -> f64 {
    (radius / (radius + altitude)).asin()
}

/// Maximum geometrically valid full field of view `2·α_max` [rad].
///
/// `radius` and `altitude` must be in the same length unit.
pub fn max_fov(radius: f64, altitude: f64) -> f64 {
    2.0 * max_half_fov(radius, altitude)
}

/// A Walker Delta constellation, with all quantities in SI units (radians, meters, seconds).
pub struct Constellation {
    /// Orbital inclination [rad]
    pub inclination: f64,
    /// Number of satellites per plane (S)
    pub satellites: u32,
    /// Number of orbital planes (P)
    pub planes: u32,
    /// Altitude above surface [m]
    pub altitude: f64,
    /// Planet radius [m]
    pub radius: f64,
    /// Standard gravitational parameter [m³/s²]
    pub mu: f64,
    /// Planet angular rotation rate [rad/s]
    pub omega: f64,
    /// Sensor full field of view (2·α) [rad]
    pub fov: f64,
}

pub struct SimulationInput {
    /// total simulated duration starting at t = 0 [s]
    pub duration: f64,
    /// sample rate in seconds
    pub dt: f64,
}

/// A (latitude, longitude) pair in radians.
pub type LatLon = (f64, f64);
/// Left/right footprint edge points on the ground.
pub type CoverageEdge = (LatLon, LatLon);

pub struct SimulationData {
    /// the groundtrack as a vector of (lat, lon) coordinates for each satellite
    pub groundtrack: Vec<Vec<LatLon>>,
    /// coverage edges as a vector of left and right (lat, lon) coordinates for each satellite
    pub coverage_edge: Vec<Vec<CoverageEdge>>,
    /// time vector
    pub time: Vec<f64>,
}

impl Constellation {
    /// Earth-central coverage half-angle σ of a single satellite footprint, in radians.
    ///
    /// σ = π/2 − α − arccos( R/(R+h) · sin(α) )
    ///
    /// Callers are expected to ensure `fov ≤ max_fov(radius, altitude)`;
    /// the arccos argument is clamped defensively.
    pub fn coverage_half_angle(&self) -> f64 {
        let alpha = self.fov * 0.5;
        let ratio = (self.radius + self.altitude) / self.radius;
        let x = (ratio * alpha.sin()).clamp(-1.0, 1.0);
        x.asin() - alpha
    }

    /// Effective equatorial swath `λ_swath` = 2σ / sin(i), in radians.
    pub fn effective_swath(&self) -> Option<f64> {
        let sin_i = self.inclination.sin();
        if sin_i.abs() < f64::EPSILON {
            return None;
        }
        Some(2.0 * self.coverage_half_angle() / sin_i)
    }

    /// Orbital period in seconds, `T_orb` = 2π · sqrt(a³/μ), with a = R + h.
    pub fn orbital_period(&self) -> f64 {
        let a = self.radius + self.altitude;
        2.0 * PI * (a.powi(3) / self.mu).sqrt()
    }

    /// Maximum revisit time at the equator, in seconds.
    ///
    /// Implements the three regimes from `walker_delta.typ`:
    /// - **Regime 1** (`λ_gap ≤ 0` and `ψ ≤ α`): swaths overlap in both directions,
    ///   `t_rev = T_orb / S`.
    /// - **Regime 2** (`λ_gap > 0` and `ψ ≤ α`): between-plane gap closed by Earth
    ///   rotation, `t_rev = λ_gap / ω + T_orb / S`.
    /// - **Regime 3** (`ψ > α`): in-plane spacing exceeds the equatorial swath, so
    ///   gaps form within a single plane. Use the three-gap theorem on the
    ///   sequence `{n·ψ mod 2π}` and pick the smallest `N` (denominator of a
    ///   continued-fraction convergent of `ψ/2π`) such that the residual
    ///   `L = |p·2π − q·ψ|` drops below the coverage threshold `α`.
    ///   Then `t_rev = (T_orb / S) · N`.
    ///
    /// Symbol convention used here:
    /// - `α = λ_swath = 2σ / sin(i)` (full equatorial coverage width)
    /// - `ψ = ω · T_orb / S` (longitude Earth rotates between in-plane passes)
    ///
    /// Returns `None` if the geometry is invalid (e.g. zero satellites/planes,
    /// `i = 0`, FOV-too-large clamp triggers, non-rotating planet with a gap),
    /// or if Regime 3 cannot achieve global coverage within a reasonable number
    /// of convergents (`ψ/2π` rational with too-large smallest gap).
    pub fn max_revisit_time(&self) -> Option<(String, f64)> {
        if self.satellites == 0 || self.planes == 0 {
            return None;
        }
        let swath = self.effective_swath()?;
        let delta_omega = 2.0 * PI / self.planes as f64;
        let lambda_gap = delta_omega - swath;

        let t_orb = self.orbital_period();
        let in_plane = t_orb / self.satellites as f64;
        let theta = self.omega * in_plane;

        // Regime 3: in-plane longitude spacing exceeds the equatorial swath
        // → successive ground tracks of one plane don't overlap, regardless
        // of `λ_gap`.
        if theta.abs() > swath {
            let mut t0 = Vec::with_capacity((self.satellites * self.planes) as usize);
            let mut phi0 = Vec::with_capacity(t0.capacity());

            let s = self.satellites as f64;
            let p = self.planes as f64;
            let f = 0.0_f64; // Walker phasing parameter F (currently hardcoded to match simulation())

            for j in 0..self.planes {
                for i in 0..self.satellites {
                    let i_f = i as f64;
                    let j_f = j as f64;

                    // Initial longitude offset on the equator:
                    // φ₀(i,j) = 2π/S · i + 2π/P · j + 2π/(S·P) · F
                    let phi_offset =
                        (2.0 * PI / s) * i_f + (2.0 * PI / p) * j_f + (2.0 * PI / (s * p)) * f;

                    // Initial time offset:
                    // t₀(i,j) = T_orb/2 - (T_orb/S · i + T_orb/(S·P) · F · i · j) mod (T_orb/2)
                    let phase_time =
                        ((t_orb / s) * i_f + (t_orb / (s * p)) * f * i_f * j_f).rem_euclid(t_orb);
                    let t_offset = (t_orb - phase_time).rem_euclid(t_orb);

                    phi0.push(phi_offset.rem_euclid(2.0 * PI));
                    t0.push(t_offset);
                }
            }

            let pg = PointGaps::new(2.0 * PI, t0, phi0, t_orb, 2.0 * PI - t_orb * self.omega);

            for g in pg.take(1000) {
                if g.largest_gap < swath {
                    return Some((String::from("light simulation"), g.new_t));
                }
            }
            return None;
        }

        // Regimes 1 & 2: in-plane swaths overlap; only inter-plane geometry matters.
        if lambda_gap <= 0.0 {
            Some((String::from("In-Plane"), in_plane)) // Regime 1
        } else {
            if self.omega.abs() < f64::EPSILON {
                return None;
            }
            Some((
                String::from("Plane to Plane Gap Fill time"),
                lambda_gap / self.omega.abs() + in_plane,
            )) // Regime 2
        }
    }

    /// Simulate satellite ground tracks and instantaneous coverage edges
    /// in a planet-fixed (ECEF-like) frame, sampled at `inp.dt` over
    /// `[0, inp.duration]`.
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
    pub fn simulation(&self, inp: &SimulationInput) -> SimulationData {
        let sigma = self.coverage_half_angle();

        let total_sats = (self.satellites as usize) * (self.planes as usize);
        let total_time = inp.duration.max(0.0);
        let dt = inp.dt.max(f64::EPSILON);
        let n_steps = (total_time / dt).ceil() as usize + 1;

        let mut groundtrack: Vec<Vec<LatLon>> = (0..total_sats)
            .map(|_| Vec::with_capacity(n_steps))
            .collect();
        let mut coverage_edge: Vec<Vec<CoverageEdge>> = (0..total_sats)
            .map(|_| Vec::with_capacity(n_steps))
            .collect();
        let mut time = Vec::with_capacity(n_steps);

        for step in 0..n_steps {
            let t = step as f64 * dt;
            time.push(t);

            for p in 0..self.planes {
                for s in 0..self.satellites {
                    let idx = (p * self.satellites + s) as usize;

                    let state = self.sat_state_at(p, s, t);
                    let r_hat = state.r_hat;
                    let c_hat = state.c_hat;

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

    /// State of one satellite at time `t`, in the planet-fixed (ECEF-like) frame.
    ///
    /// All vectors are in meters / dimensionless as noted. The triad
    /// `(r_hat, t_hat, c_hat)` is the local nadir/along-track/cross-track
    /// orthonormal basis at the satellite, with `c_hat = t_hat × r_hat`
    /// pointing to the right of motion.
    pub(crate) fn sat_state_at(&self, plane: u32, slot: u32, t: f64) -> SatState {
        const F: u32 = 0; // Walker phasing parameter (hardcoded, matches simulation())

        let two_pi = 2.0 * PI;
        let a = self.radius + self.altitude;
        let n = two_pi / self.orbital_period();
        let i = self.inclination;

        let d_raan = two_pi / self.planes.max(1) as f64;
        let d_in_plane = two_pi / self.satellites.max(1) as f64;
        let d_between = if self.satellites > 0 && self.planes > 0 {
            two_pi * F as f64 / (self.satellites * self.planes) as f64
        } else {
            0.0
        };

        let raan = plane as f64 * d_raan;
        let nu = slot as f64 * d_in_plane + plane as f64 * d_between + n * t;
        let theta = self.omega * t;

        let (sn, cn) = nu.sin_cos();
        let r_pf = [a * cn, a * sn, 0.0];
        let v_pf = [-a * n * sn, a * n * cn, 0.0];

        let r_eci = rot_z(rot_x(r_pf, i), raan);
        let v_eci = rot_z(rot_x(v_pf, i), raan);

        let r_ecef = rot_z(r_eci, -theta);
        let v_rot = rot_z(v_eci, -theta);
        let v_ecef = [
            v_rot[0] - (-self.omega * r_ecef[1]),
            v_rot[1] - (self.omega * r_ecef[0]),
            v_rot[2],
        ];

        let r_hat = normalize(r_ecef);
        let vr = dot(v_ecef, r_hat);
        let v_tan = [
            v_ecef[0] - vr * r_hat[0],
            v_ecef[1] - vr * r_hat[1],
            v_ecef[2] - vr * r_hat[2],
        ];
        let t_hat = normalize(v_tan);
        let c_hat = normalize(cross(t_hat, r_hat));

        SatState {
            r_ecef,
            r_hat,
            t_hat,
            c_hat,
        }
    }
}

/// Position and local orthonormal triad of one satellite in the planet-fixed frame.
pub(crate) struct SatState {
    #[expect(dead_code, reason = "kept for completeness of the local frame")]
    pub r_ecef: [f64; 3],
    pub r_hat: [f64; 3],
    #[expect(dead_code, reason = "kept for completeness of the local frame")]
    pub t_hat: [f64; 3],
    pub c_hat: [f64; 3],
}

// --- small vector helpers (3D, f64) -----------------------------------------

pub(crate) fn rot_x(v: [f64; 3], a: f64) -> [f64; 3] {
    let (s, c) = a.sin_cos();
    [v[0], c * v[1] - s * v[2], s * v[1] + c * v[2]]
}

pub(crate) fn rot_z(v: [f64; 3], a: f64) -> [f64; 3] {
    let (s, c) = a.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1], v[2]]
}

pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn normalize(v: [f64; 3]) -> [f64; 3] {
    let m = dot(v, v).sqrt();
    if m < f64::EPSILON {
        [0.0, 0.0, 0.0]
    } else {
        [v[0] / m, v[1] / m, v[2] / m]
    }
}

/// Convert a unit vector (in a planet-fixed frame) to (lat, lon) in radians.
pub(crate) fn to_lat_lon(u: [f64; 3]) -> (f64, f64) {
    let lat = u[2].clamp(-1.0, 1.0).asin();
    let lon = u[1].atan2(u[0]);
    (lat, lon)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Gap {
    new_t: f64,
    new_phi: f64,
    largest_gap: f64,
}

#[derive(Debug)]
struct PointGaps {
    length: f64,
    phi: Vec<f64>,
    t: Vec<f64>,
    t0: Vec<f64>,
    phi0: Vec<f64>,
    dt: f64,
    dphi: f64,
    i: usize,
}

impl PointGaps {
    fn new(length: f64, t0: Vec<f64>, phi0: Vec<f64>, delta_t: f64, delta_phi: f64) -> Self {
        assert_eq!(
            t0.len(),
            phi0.len(),
            "delta_t and delta_phi must have the same length"
        );

        let mut pairs: Vec<(f64, f64)> = t0.into_iter().zip(phi0).collect();
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));

        let (t0, phi0): (Vec<f64>, Vec<f64>) = pairs.into_iter().unzip();

        Self {
            length,
            // No sentinels: `largest_gap` treats `phi` as points on a circle
            // of circumference `length` and computes the wraparound gap
            // explicitly. Seeding `[0, length]` here would split the
            // wraparound arc into two linear pieces and systematically
            // under-report the largest gap.
            phi: Vec::new(),
            t: vec![t0[0]],
            t0,
            phi0,
            dt: delta_t,
            dphi: delta_phi,
            i: 0,
        }
    }
}

impl Iterator for PointGaps {
    type Item = Gap;

    fn next(&mut self) -> Option<Gap> {
        let n = self.phi0.len();
        let k = self.i / n;
        let i = self.i % n;
        let new_t = self.t0[i] + self.dt * k as f64;
        let new_phi = (self.phi0[i] + self.dphi * k as f64) % self.length;
        self.t.push(new_t);
        self.phi.push(new_phi);
        let lg = largest_gap(&mut self.phi, self.length).expect("no gap found");
        self.i += 1;
        Some(Gap {
            new_t,
            new_phi,
            largest_gap: lg,
        })
    }
}

/// Largest gap between consecutive points on a circle of circumference
/// `length`. Treats `v` as positions in `[0, length)` and includes the
/// wraparound arc `length - v[n-1] + v[0]`.
///
/// Returns `None` for an empty input. For a single point, returns `length`
/// (the entire circle except that point is one empty arc).
fn largest_gap(v: &mut [f64], length: f64) -> Option<f64> {
    if v.is_empty() {
        return None;
    }

    v.sort_by(|a, b| a.total_cmp(b));

    if v.len() == 1 {
        return Some(length);
    }

    // Wraparound arc: from the last sorted point, around through `length`/0,
    // back to the first sorted point.
    let mut max_gap = length - v[v.len() - 1] + v[0];
    for w in v.windows(2) {
        let gap = w[1] - w[0];
        if gap > max_gap {
            max_gap = gap;
        }
    }

    Some(max_gap)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Rough Earth-ish parameters for smoke tests.
    fn earth_like(inclination_deg: f64) -> Constellation {
        Constellation {
            inclination: inclination_deg.to_radians(),
            satellites: 4,
            planes: 3,
            altitude: 550_000.0,        // 550 km
            radius: 6_371_000.0,        // 6371 km
            mu: 3.986_004_418e14,       // m^3/s^2
            omega: 7.292_115e-5,        // rad/s
            fov: 60.0_f64.to_radians(), // 60° full FoV
        }
    }

    #[test]
    fn simulation_smoke_basic_shape_and_bounds() {
        let c = earth_like(53.0);
        let inp = SimulationInput {
            duration: c.orbital_period(),
            dt: 60.0, // 1-minute samples
        };
        let data = c.simulation(&inp);

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
                assert!(lat.abs() <= i + 1e-3, "|lat|={lat} > i={i}");
                assert!(lon.is_finite());
                assert!(lon.abs() <= PI + 1e-5);
            }
        }
    }

    #[test]
    fn largest_gap_is_correct() {
        // Five points on a circle of circumference 1.0. The biggest empty
        // arc is between 0.3 and 0.9 (length 0.6); the wraparound from 1.0
        // back to 0.0 is degenerate (both are the same point on the circle
        // — gap = 0).
        let mut v = vec![0.0, 0.2, 0.3, 0.9, 1.0];
        let lg = largest_gap(&mut v, 1.0).expect("non-empty input");
        assert!((lg - 0.6).abs() < 1e-9, "got {lg}");
    }

    #[test]
    fn largest_gap_uses_circular_wraparound() {
        // Three satellites at 90°, 180°, 270° on a 360° circle. The largest
        // empty arc wraps through 0°: from 270° forward to 90°, length 180°.
        // A non-circular implementation that splits the wraparound at 0/360
        // would report 90° (the largest of the in-range diffs).
        let mut v = vec![90.0, 180.0, 270.0];
        let lg = largest_gap(&mut v, 360.0).expect("non-empty input");
        assert!((lg - 180.0).abs() < 1e-9, "got {lg}");
    }

    #[test]
    fn largest_gap_single_point_is_full_circle() {
        let mut v = vec![42.0];
        let lg = largest_gap(&mut v, 360.0).expect("non-empty input");
        assert!((lg - 360.0).abs() < 1e-9, "got {lg}");
    }

    #[test]
    fn largest_gap_empty_is_none() {
        let mut v: Vec<f64> = vec![];
        assert!(largest_gap(&mut v, 360.0).is_none());
    }

    #[test]
    fn point_gaps_reports_circular_wraparound() {
        // Regression: with three satellites at 90°/180°/270° the iterator
        // should report largest_gap = 180° on the third step (the empty arc
        // wraps through 0°). The pre-fix sentinel-based implementation
        // reported 90° here because the wraparound was split by phantom
        // points at 0 and 360.
        let pg = PointGaps::new(
            360.0,
            vec![0.0, 1.0, 2.0],
            vec![90.0, 180.0, 270.0],
            // dt/dphi are irrelevant for the first n steps — we only inspect
            // the gap after all three real points have been added.
            10.0,
            0.0,
        );
        let third = pg.take(3).last().expect("three steps");
        assert!(
            (third.largest_gap - 180.0).abs() < 1e-9,
            "got {}",
            third.largest_gap
        );
    }

    #[test]
    fn simulation_equatorial_orbit_stays_on_equator() {
        let c = Constellation {
            inclination: 0.0,
            ..earth_like(0.0)
        };
        let inp = SimulationInput {
            duration: c.orbital_period(),
            dt: 30.0,
        };
        // Note: inclination = 0 makes effective_swath() undefined, but the
        // simulation itself doesn't depend on it.
        let data = c.simulation(&inp);
        for track in &data.groundtrack {
            for (lat, _lon) in track {
                assert!(lat.abs() < 1e-4, "equatorial track left equator: lat={lat}");
            }
        }
    }

    #[test]
    fn coverage_edges_are_at_central_angle_sigma() {
        let c = earth_like(53.0);
        let sigma = c.coverage_half_angle();
        let inp = SimulationInput {
            duration: c.orbital_period() * 0.25,
            dt: 120.0,
        };
        let data = c.simulation(&inp);

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
                    "left edge angle {d_left} vs σ {sigma}"
                );
                assert!(
                    (d_right - sigma).abs() < 1e-3,
                    "right edge angle {d_right} vs σ {sigma}"
                );
            }
        }
    }

    fn ll_to_unit(lat: f64, lon: f64) -> [f64; 3] {
        let (slat, clat) = lat.sin_cos();
        let (slon, clon) = lon.sin_cos();
        [clat * clon, clat * slon, slat]
    }

    #[test]
    fn point_gaps_correct() {
        let pg = PointGaps::new(
            360.0,
            vec![0.0, 5.0, 10.0, 12.5, 2.5, 7.5],
            vec![0.0, 120.0, 240.0, 60.0, 180.0, 300.0],
            15.0,
            180.0,
        );
        let actual: Vec<(f64, f64, f64)> = pg
            .take(12)
            .map(|g| (g.new_t, g.new_phi, g.largest_gap))
            .collect();

        let expected = vec![
            (0.0, 0.0, 360.0),
            (2.5, 180.0, 180.0),
            (5.0, 120.0, 180.0),
            (7.5, 300.0, 120.0),
            (10.0, 240.0, 120.0),
            (12.5, 60.0, 60.0),
            (15.0, 180.0, 60.0),
            (17.5, 0.0, 60.0),
            (20.0, 300.0, 60.0),
            (22.5, 120.0, 60.0),
            (25.0, 60.0, 60.0),
            (27.5, 240.0, 60.0),
        ];

        assert_eq!(actual.len(), expected.len());
        for ((at, aphi, agap), (et, ephi, egap)) in actual.into_iter().zip(expected) {
            assert!((at - et).abs() < 1e-9, "t: got {at}, expected {et}");
            assert!(
                (aphi - ephi).abs() < 1e-9,
                "t: {at}, phi: got {aphi}, expected {ephi}"
            );
            assert!(
                (agap - egap).abs() < 1e-9,
                "t: {at}, gap: got {agap}, expected {egap}"
            );
        }
    }

    #[test]
    fn max_revisit_regime_3_smoke() {
        let c = Constellation {
            inclination: 60.0_f64.to_radians(),
            satellites: 2,
            planes: 2,
            altitude: 500_000.0,
            radius: 6_371_000.0,
            mu: 3.986_004_418e14,
            omega: (15.0 / (24.0f64 * 3600.0)).to_radians(),
            fov: 20.0_f64.to_radians(),
        };

        c.max_revisit_time().expect("Regime 3 should resolve");
    }
}
