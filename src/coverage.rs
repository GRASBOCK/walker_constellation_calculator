//! Per-satellite coverage rasterization.
//!
//! Produces a float-valued raster ([`CoverageMap`]) on an equirectangular
//! projection of the planet, where each pixel stores the **earliest time**
//! (in seconds) at which it was inside the satellite's instantaneous
//! footprint. Pixels never covered hold `f64::INFINITY`.
//!
//! Temporal aliasing is avoided by sampling the satellite state at a
//! sub-step `dt_rast` chosen so the footprint advances at most a fraction
//! `k` of its own diameter per sub-step (currently `k = 0.1`). Override
//! via [`RasterizeOptions::dt_rast`].
//!
//! Pole-cap wrap and dateline wrap are both handled.

use std::f64::{self, consts::PI};

use crate::constellation::Constellation;

/// A float-valued raster on the equirectangular projection of the planet.
///
/// Pixel `(x, y)` covers the lat/lon cell centered at:
///   lon = (x + 0.5) / width  · 2π − π
///   lat = π/2 − (y + 0.5) / height · π
pub struct CoverageMap {
    pub width: usize,
    pub height: usize,
    /// Time of first coverage per pixel [s]. `f64::INFINITY` = never covered.
    pub data: Vec<f64>,
}

/// Options controlling how a coverage map is rasterized.
pub struct RasterizeOptions {
    pub width: usize,
    pub height: usize,
    /// Supersample step in seconds. `None` = auto-pick from footprint size
    /// and ground speed so the footprint advances ≲ `0.1 · (2σ)` per
    /// sub-step. Pass `Some(dt)` to override.
    pub dt_rast: Option<f64>,
}

impl CoverageMap {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![f64::INFINITY; width * height],
        }
    }

    /// Element-wise minimum. Used to fold N per-satellite maps into a
    /// single constellation-wide "earliest any satellite covered this pixel" map.
    pub fn combine_min(&mut self, other: &Self) {
        assert_eq!(self.width, other.width, "width mismatch");
        assert_eq!(self.height, other.height, "height mismatch");
        for (a, b) in self.data.iter_mut().zip(&other.data) {
            if *b < *a {
                *a = *b;
            }
        }
    }

    /// Rasterize the coverage of one satellite over `[t_start, t_end]` by
    /// supersampling the spherical-cap footprint.
    #[expect(
        clippy::cast_possible_wrap,
        reason = "pixel coordinates are well within i64 range"
    )]
    pub fn from_satellite(
        c: &Constellation,
        plane: u32,
        slot: u32,
        t_start: f64,
        t_end: f64,
        opts: &RasterizeOptions,
    ) -> Self {
        let mut map = Self::new(opts.width, opts.height);

        let sigma = c.coverage_half_angle();
        if !(sigma > 0.0 && t_end > t_start) || opts.width == 0 || opts.height == 0 {
            return map;
        }

        // Auto-pick the supersample step.
        //
        // We want the satellite footprint to advance at most k · (2σ) along
        // its ground track per sub-step. Angular ground speed of the
        // sub-satellite point (worst case): n + |ω|. Setting
        //   dt_rast · (n + |ω|) ≤ k · 2σ
        // → dt_rast ≤ 2kσ / (n + |ω|).
        let dt_rast = opts.dt_rast.unwrap_or_else(|| {
            let n = 2.0 * PI / c.orbital_period();
            let denom = n + c.omega.abs();
            let k = 0.1;
            if denom > 0.0 {
                (2.0 * k * sigma / denom).max(1e-3)
            } else {
                1.0
            }
        });
        let dt_rast = dt_rast.max(f64::EPSILON);

        let cos_sigma = sigma.cos();
        let w = opts.width;
        let h = opts.height;
        let wf = w as f64;
        let hf = h as f64;

        let mut t = t_start;
        while t <= t_end {
            let state = c.sat_state_at(plane, slot, t);
            let r = state.r_hat;

            // Sub-satellite (lat, lon)
            let sub_lat = r[2].clamp(-1.0, 1.0).asin();
            let sub_lon = r[1].atan2(r[0]);

            // Lat bounding box of the cap, clipped to ±π/2.
            let lat_lo = (sub_lat - sigma).max(-0.5 * PI);
            let lat_hi = (sub_lat + sigma).min(0.5 * PI);

            // Pixel y-range. y increases as lat decreases.
            let y_top = ((0.5 * PI - lat_hi) / PI * hf).floor().max(0.0) as usize;
            let y_bot_excl =
                (((0.5 * PI - lat_lo) / PI * hf).ceil() as i64).clamp(0, h as i64) as usize;

            // If the cap reaches a pole, the longitude bound diverges; cover full width.
            let pole_wrap = sub_lat.abs() + sigma >= 0.5 * PI;

            for y in y_top..y_bot_excl {
                let lat = 0.5 * PI - (y as f64 + 0.5) / hf * PI;
                let (slat, clat) = lat.sin_cos();

                // Pixel x-range, possibly straddling the dateline (handled with rem_euclid).
                let (x_start, x_end) = if pole_wrap || clat < 1e-6 {
                    (0i64, w as i64)
                } else {
                    // Loose lon half-width at this latitude. Wider than necessary
                    // near the cap's lat extremes; the per-pixel dot-product test
                    // rejects pixels outside the cap.
                    let dlon = (sigma / clat).min(PI);
                    let lon_lo = sub_lon - dlon;
                    let lon_hi = sub_lon + dlon;
                    let x0 = ((lon_lo + PI) / (2.0 * PI) * wf).floor() as i64;
                    let x1 = ((lon_hi + PI) / (2.0 * PI) * wf).ceil() as i64 + 1;
                    // Clamp span to at most one full revolution.
                    let x1 = if x1 - x0 > w as i64 {
                        x0 + w as i64
                    } else {
                        x1
                    };
                    (x0, x1)
                };

                let row = y * w;
                for xi in x_start..x_end {
                    let x = xi.rem_euclid(w as i64) as usize;
                    let lon = (x as f64 + 0.5) / wf * 2.0 * PI - PI;
                    let (slon, clon) = lon.sin_cos();
                    let p_hat = [clat * clon, clat * slon, slat];
                    let dotp = p_hat[0] * r[0] + p_hat[1] * r[1] + p_hat[2] * r[2];
                    if dotp >= cos_sigma {
                        let i = row + x;
                        if t < map.data[i] {
                            map.data[i] = t;
                        }
                    }
                }
            }

            t += dt_rast;
        }

        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn earth_like(inclination_deg: f64) -> Constellation {
        Constellation {
            inclination: inclination_deg.to_radians(),
            satellites: 1,
            planes: 1,
            altitude: 550_000.0,
            radius: 6_371_000.0,
            mu: 3.986_004_418e14,
            omega: 7.292_115e-5,
            fov: 60.0_f64.to_radians(),
        }
    }

    #[test]
    fn coverage_low_inclination_fills_only_equatorial_band() {
        let c = earth_like(10.0);
        let sigma = c.coverage_half_angle();
        let lat_max = c.inclination + sigma + 0.05;

        let opts = RasterizeOptions {
            width: 360,
            height: 180,
            dt_rast: None,
        };
        let map = CoverageMap::from_satellite(&c, 0, 0, 0.0, c.orbital_period(), &opts);

        let mut covered = 0usize;
        for y in 0..map.height {
            let lat = 0.5 * PI - (y as f64 + 0.5) / map.height as f64 * PI;
            for x in 0..map.width {
                if map.data[y * map.width + x].is_finite() {
                    covered += 1;
                    assert!(
                        lat.abs() <= lat_max,
                        "covered pixel outside band: lat={}°, lat_max={}°",
                        lat.to_degrees(),
                        lat_max.to_degrees()
                    );
                }
            }
        }
        assert!(covered > 0, "no pixels were covered");
    }

    #[test]
    fn finer_supersample_yields_superset_coverage() {
        let c = earth_like(53.0);
        let opts_coarse = RasterizeOptions {
            width: 180,
            height: 90,
            dt_rast: Some(60.0),
        };
        let opts_fine = RasterizeOptions {
            width: 180,
            height: 90,
            dt_rast: Some(15.0),
        };

        let map_c = CoverageMap::from_satellite(&c, 0, 0, 0.0, c.orbital_period(), &opts_coarse);
        let map_f = CoverageMap::from_satellite(&c, 0, 0, 0.0, c.orbital_period(), &opts_fine);

        // Every pixel covered in the coarse map must also be covered in the fine map,
        // with an equal-or-earlier time (finer sampling can only add coverage or
        // refine an earlier hit time).
        for (a, b) in map_c.data.iter().zip(&map_f.data) {
            if a.is_finite() {
                assert!(
                    b.is_finite() && *b <= *a + 1e-3,
                    "fine map missing/later: coarse={a} fine={b}"
                );
            }
        }
    }

    /// Brute-force ground truth: a pixel center is covered at some sampled
    /// time iff the angular distance from the pixel to the sub-sat point is
    /// `≤ σ`. Using the same `dt` as the rasterizer isolates geometry from
    /// temporal aliasing.
    #[expect(
        clippy::too_many_arguments,
        reason = "ground-truth helper takes the full sample parameters explicitly"
    )]
    fn brute_force_covered(
        c: &Constellation,
        plane: u32,
        slot: u32,
        t_start: f64,
        t_end: f64,
        dt: f64,
        lat: f64,
        lon: f64,
    ) -> bool {
        let sigma = c.coverage_half_angle();
        let cos_sigma = sigma.cos();
        let (slat, clat) = lat.sin_cos();
        let (slon, clon) = lon.sin_cos();
        let p = [clat * clon, clat * slon, slat];
        let mut t = t_start;
        while t <= t_end {
            let r = c.sat_state_at(plane, slot, t).r_hat;
            if p[0] * r[0] + p[1] * r[1] + p[2] * r[2] >= cos_sigma {
                return true;
            }
            t += dt;
        }
        false
    }

    /// Equatorial orbit (i = 0) keeps the sub-sat point on the equator, so
    /// the cap covers exactly the band |lat| ≤ σ over one full ground-track
    /// revolution. Rows well inside the band must be fully covered; rows
    /// well outside must be entirely uncovered.
    #[test]
    fn equatorial_orbit_covers_exactly_band_within_sigma() {
        let c = earth_like(0.0);
        let sigma = c.coverage_half_angle();

        // In ECEF the sub-sat longitude drifts at (n - ω), so one full lap
        // takes 2π / |n - ω|.
        let n = 2.0 * PI / c.orbital_period();
        let lap = 2.0 * PI / (n - c.omega).abs();
        let opts = RasterizeOptions {
            width: 360,
            height: 180,
            dt_rast: None,
        };
        let map = CoverageMap::from_satellite(&c, 0, 0, 0.0, lap, &opts);

        let hf = map.height as f64;
        let margin = 0.5 * PI / hf; // one pixel row of slack
        for y in 0..map.height {
            let lat = 0.5 * PI - (y as f64 + 0.5) / hf * PI;
            let row_all = (0..map.width).all(|x| map.data[y * map.width + x].is_finite());
            let row_none = (0..map.width).all(|x| map.data[y * map.width + x].is_infinite());
            if lat.abs() < sigma - margin {
                assert!(
                    row_all,
                    "row at lat={}° (inside |lat|<σ={}°) not fully covered",
                    lat.to_degrees(),
                    sigma.to_degrees()
                );
            } else if lat.abs() > sigma + margin {
                assert!(
                    row_none,
                    "row at lat={}° (outside |lat|>σ={}°) has covered pixels",
                    lat.to_degrees(),
                    sigma.to_degrees()
                );
            }
        }
    }

    /// Validate that the bounding-box + dot-product rasterization agrees with
    /// the brute-force per-pixel test on a representative subgrid — no false
    /// positives outside the cap and no false negatives inside (including
    /// across the dateline and near the poles).
    #[test]
    fn rasterizer_matches_brute_force_on_subgrid() {
        let c = earth_like(53.0);
        let dt = 60.0;
        let opts = RasterizeOptions {
            width: 90,
            height: 45,
            dt_rast: Some(dt),
        };
        let map = CoverageMap::from_satellite(&c, 0, 0, 0.0, c.orbital_period(), &opts);

        let hf = map.height as f64;
        let wf = map.width as f64;
        let mut checked = 0usize;
        for y in (0..map.height).step_by(3) {
            for x in (0..map.width).step_by(5) {
                let lat = 0.5 * PI - (y as f64 + 0.5) / hf * PI;
                let lon = (x as f64 + 0.5) / wf * 2.0 * PI - PI;
                let raster = map.data[y * map.width + x].is_finite();
                let truth = brute_force_covered(&c, 0, 0, 0.0, c.orbital_period(), dt, lat, lon);
                assert_eq!(
                    raster,
                    truth,
                    "mismatch at (x={}, y={}) lat={}° lon={}°",
                    x,
                    y,
                    lat.to_degrees(),
                    lon.to_degrees()
                );
                checked += 1;
            }
        }
        assert!(checked > 0);
    }

    /// A polar orbit (i = 90°) passes essentially over the pole, so every
    /// pixel in the top pixel row must be covered. This exercises the
    /// `pole_wrap` branch: without it, the longitude bounding box would
    /// collapse near the pole and miss most of the row.
    #[test]
    fn polar_orbit_covers_the_pole_row() {
        let c = earth_like(90.0);
        let opts = RasterizeOptions {
            width: 180,
            height: 90,
            dt_rast: None,
        };
        let map = CoverageMap::from_satellite(&c, 0, 0, 0.0, c.orbital_period(), &opts);
        for x in 0..map.width {
            assert!(
                map.data[x].is_finite(),
                "pole-row pixel x={x} not covered by polar orbit"
            );
        }
    }

    #[test]
    fn combine_min_basics() {
        let mut a = CoverageMap::new(4, 2);
        let mut b = CoverageMap::new(4, 2);
        a.data[0] = 10.0;
        a.data[1] = f64::INFINITY;
        b.data[0] = 5.0;
        b.data[1] = 7.0;
        a.combine_min(&b);
        assert_eq!(a.data[0], 5.0);
        assert_eq!(a.data[1], 7.0);
    }
}
