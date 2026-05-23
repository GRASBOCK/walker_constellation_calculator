use crate::constellation::{self, Constellation, SimulationInput};
use crate::coverage::{CoverageMap, RasterizeOptions};

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)] // if we add new fields, give them default values when deserializing old state
pub struct App {
    // Example stuff:
    label: String,

    inclination: f32,
    satellites: f32,
    planes: f32,
    altitude: f32,
    radius: f32,
    mu: f32,
    omega: f32,
    fov: f32,

    // Simulation parameters (user-facing units: hours / seconds)
    sim_timespan_h: f32,
    sim_max_prediction_h: f32,
    sim_dt_s: f32,

    // Coverage map output size
    coverage_width: usize,
    coverage_height: usize,

    // Cached rasterized texture (not persisted)
    #[serde(skip)]
    coverage_texture: Option<egui::TextureHandle>,
    /// Maximum finite "time of first coverage" from the last computed map [s].
    /// Acts as the simulated worst-case revisit time over the globe (excluding
    /// pixels that were never covered).
    #[serde(skip)]
    coverage_max_time_s: Option<f32>,
    /// Number of pixels in the last computed map that were never covered.
    #[serde(skip)]
    coverage_uncovered_pixels: Option<usize>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            // Example stuff:
            label: "Hello World!".to_owned(),
            inclination: 60.0,
            satellites: 16.0,
            planes: 2.0,
            altitude: 500.0,
            mu: 3.986_004_5E14,
            radius: 6378.1,
            omega: 360.0 / 24.0,
            fov: 20.0,

            sim_timespan_h: 24.0,
            sim_max_prediction_h: 72.0,
            sim_dt_s: 60.0,

            coverage_width: 720,
            coverage_height: 360,
            coverage_texture: None,
            coverage_max_time_s: None,
            coverage_uncovered_pixels: None,
        }
    }
}

impl App {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.
        cc.egui_ctx.set_visuals(egui::Visuals::light());

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        if let Some(storage) = cc.storage {
            eframe::get_value(storage, eframe::APP_KEY).unwrap_or_default()
        } else {
            Default::default()
        }
    }
}

impl eframe::App for App {
    /// Called by the framework to save state before shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's
            ui.heading("Walker Constellation Calculator");

            ui.horizontal(|ui| {
                ui.label("inclination [°]:")
                    .on_hover_text("orbital inclincation in degrees");
                ui.add(
                    egui::DragValue::new(&mut self.inclination)
                        .speed(0.5)
                        .range(0.0..=90.0),
                );
                ui.label("altitude [km]: ")
                    .on_hover_text("Height above the planets surface");
                ui.add(egui::DragValue::new(&mut self.altitude).speed(0.5));
            });
            ui.horizontal(|ui| {
                ui.label("Satellites:")
                    .on_hover_text("Number of Satellites per Plane");
                ui.add(egui::DragValue::new(&mut self.satellites).speed(1.0));
                ui.label("Planes:")
                    .on_hover_text("Number of Planes in Constellation");
                ui.add(egui::DragValue::new(&mut self.planes).speed(1.0));
                ui.label(format!(
                    "Total Satellites: {:.0}",
                    self.planes * self.satellites
                ))
                .on_hover_text("Total number of Satellites of Satellites in Constellation");
            });
            ui.horizontal(|ui| {
                ui.label("µ [m³/s²]: ")
                    .on_hover_text("Standard Gravitational Parameter [m³/s²]");
                ui.add(egui::DragValue::new(&mut self.mu).speed(1.0));
                ui.label("ω [°/h]: ")
                    .on_hover_text("Rotation Speed of Planet in degrees per hour [°/h]");
                ui.add(egui::DragValue::new(&mut self.omega).speed(1.0));
                ui.label("R [km]: ")
                    .on_hover_text("Radius of the planet in km");
                ui.add(egui::DragValue::new(&mut self.radius).speed(1.0));

                let mut selected_planet: Option<(&str, f32, f32, f32)> = None;
                ui.menu_button(egui::RichText::new("🌍"), |ui| {
                    if ui.button("Mercury").clicked() {
                        selected_planet = Some(("Mercury", 22032E9, 0.00220968, 2439.7));
                        ui.close();
                    }
                    if ui.button("Venus").clicked() {
                        selected_planet = Some(("Venus", 324859E9, -0.00107712, 6051.8));
                        ui.close();
                    }
                    if ui.button("Earth").clicked() {
                        selected_planet = Some(("Earth", 3.986_004_5E14, 15.0, 6378.1));
                        ui.close();
                    }
                    if ui.button("Mars").clicked() {
                        selected_planet = Some(("Mars", 42828E9, 0.255168, 3389.5));
                        ui.close();
                    }
                    if ui.button("Jupiter").clicked() {
                        selected_planet = Some(("Jupiter", 1.266_865_4E17, 0.6330708, 69911.0));
                        ui.close();
                    }
                    if ui.button("Saturn").clicked() {
                        selected_planet = Some(("Saturn", 3.793_119E16, 0.589644, 58232.0));
                        ui.close();
                    }
                    if ui.button("Uranus").clicked() {
                        selected_planet = Some(("Uranus", 5793939E9, -0.36432, 25362.0));
                        ui.close();
                    }
                    if ui.button("Neptune").clicked() {
                        selected_planet = Some(("Neptune", 6836529E9, 0.38988, 24622.0));
                        ui.close();
                    }
                });

                if let Some((_planet, mu, omega, radius)) = selected_planet {
                    self.mu = mu;
                    self.omega = omega;
                    self.radius = radius;
                }
            });
            ui.horizontal(|ui| {
                let max_fov_deg = constellation::max_fov(self.radius, self.altitude).to_degrees();
                // Re-clamp in case altitude/radius shrank since fov was last set
                // (e.g. via the planet menu or altitude DragValue).
                if self.fov > max_fov_deg {
                    self.fov = max_fov_deg;
                }
                ui.label("FoV [°]: ").on_hover_text(format!(
                    "Field of view in degrees (max {max_fov_deg:.3}° at current altitude)"
                ));
                ui.add(
                    egui::DragValue::new(&mut self.fov)
                        .speed(0.5)
                        .range(0.0..=max_fov_deg),
                );
            });

            ui.separator();

            // input validation and clamping
            self.inclination = self.inclination.clamp(0.0, 90.0);
            self.altitude = self.altitude.max(0.0);
            self.satellites = self.satellites.max(1.0).round();
            self.planes = self.planes.max(1.0).round();
            self.mu = self.mu.max(0.0);
            self.omega = self.omega.max(0.0);
            self.radius = self.radius.max(0.0);
            self.fov = self.fov.max(0.0);

            let constellation = Constellation {
                inclination: self.inclination.to_radians(),
                satellites: self.satellites as u32,
                planes: self.planes as u32,
                altitude: self.altitude * 1000.0,
                radius: self.radius * 1000.0,
                mu: self.mu,
                omega: self.omega.to_radians() / 3600.0,
                fov: self.fov.to_radians(),
            };

            ui.label("Analytical Solution");
            ui.horizontal(|ui| {
                match constellation.max_revisit_time() {
                    Some(seconds) => ui.label(format!(
                        "Maximum revisit time: {:.3} hours ({:.3} days)",
                        seconds / 3600.0,
                        seconds / 86400.0,
                    )),
                    None => ui.label("Maximum revisit time: N/A (invalid geometry or parameters)"),
                };
            });

            ui.separator();
            ui.label("Simulation");

            // input simulation parameters
            ui.horizontal(|ui| {
                ui.label("Timespan [h]:").on_hover_text(
                    "Length of the visible time window (rolling history shown in plots)",
                );
                ui.add(
                    egui::DragValue::new(&mut self.sim_timespan_h)
                        .speed(0.1)
                        .range(0.0..=f32::INFINITY),
                );
                ui.label("Prediction [h]:").on_hover_text(
                    "How far into the future the simulation runs beyond the visible window",
                );
                ui.add(
                    egui::DragValue::new(&mut self.sim_max_prediction_h)
                        .speed(0.1)
                        .range(0.0..=f32::INFINITY),
                );
                ui.label("dt [s]:")
                    .on_hover_text("Simulation sample step in seconds");
                ui.add(
                    egui::DragValue::new(&mut self.sim_dt_s)
                        .speed(1.0)
                        .range(0.001..=f32::INFINITY),
                );
            });

            // input validation
            self.sim_timespan_h = self.sim_timespan_h.max(0.0);
            self.sim_max_prediction_h = self.sim_max_prediction_h.max(0.0);
            self.sim_dt_s = self.sim_dt_s.max(0.001);

            let inp = SimulationInput {
                timespan: self.sim_timespan_h * 3600.0,
                max_predicition_time: self.sim_max_prediction_h * 3600.0,
                dt: self.sim_dt_s,
            };
            let total_sim_time = inp.timespan + inp.max_predicition_time;

            // -- Coverage map ------------------------------------------------
            ui.horizontal(|ui| {
                if ui.button("Compute coverage map").clicked() {
                    let t_end = total_sim_time;
                    let opts = RasterizeOptions {
                        width: self.coverage_width,
                        height: self.coverage_height,
                        dt_rast: None,
                    };

                    let mut combined = CoverageMap::new(self.coverage_width, self.coverage_height);
                    for p in 0..constellation.planes {
                        for s in 0..constellation.satellites {
                            let m = CoverageMap::from_satellite(
                                &constellation,
                                p,
                                s,
                                0.0,
                                t_end,
                                &opts,
                            );
                            combined.combine_min(&m);
                        }
                    }

                    let img = coverage_to_color_image(&combined);
                    self.coverage_texture = Some(ui.ctx().load_texture(
                        "coverage_map",
                        img,
                        egui::TextureOptions::NEAREST,
                    ));

                    // Stats from the freshly computed map.
                    let mut t_max: f32 = 0.0;
                    let mut n_uncov: usize = 0;
                    let mut any_finite = false;
                    for v in &combined.data {
                        if v.is_finite() {
                            any_finite = true;
                            if *v > t_max {
                                t_max = *v;
                            }
                        } else {
                            n_uncov += 1;
                        }
                    }
                    self.coverage_max_time_s = if any_finite { Some(t_max) } else { None };
                    self.coverage_uncovered_pixels = Some(n_uncov);
                }
                ui.label(format!(
                    "Map size: {}×{}",
                    self.coverage_width, self.coverage_height
                ))
                .on_hover_text("Equirectangular projection (full planet)");
            });

            // Simulated max revisit (max time-of-first-coverage over all
            // covered pixels), plus a count of pixels never covered.
            ui.horizontal(|ui| {
                ui.label("Simulated max revisit:");
                match self.coverage_max_time_s {
                    Some(seconds) => {
                        ui.label(format!(
                            "{:.3} h ({:.3} days)",
                            seconds / 3600.0,
                            seconds / 86400.0,
                        ))
                        .on_hover_text(
                            "Worst-case time-of-first-coverage across all covered pixels.\n\
                             Equals the longest wait any covered point on the globe\n\
                             had to endure before being seen for the first time.",
                        );
                    }
                    None => {
                        ui.label("(no map computed yet)");
                    }
                }
            });

            if let Some(tex) = &self.coverage_texture {
                let avail = ui.available_width();
                let aspect = tex.size_vec2().x / tex.size_vec2().y;
                let w = avail.min(tex.size_vec2().x);
                let h = w / aspect;
                ui.add(
                    egui::Image::from_texture((tex.id(), egui::vec2(w, h)))
                        .fit_to_exact_size(egui::vec2(w, h)),
                );
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::github_link_file!(
                        "https://github.com/GRASBOCK/walker_constellation_calculator/tree/main",
                        "Source code"
                    ));

                    egui::warn_if_debug_build(ui);
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label("Powered by ");
                    ui.hyperlink_to("egui", "https://github.com/emilk/egui");
                    ui.label(" and ");
                    ui.hyperlink_to(
                        "eframe",
                        "https://github.com/emilk/egui/tree/master/crates/eframe",
                    );
                    ui.label(".");
                    ui.spacing_mut().item_spacing.x = 1.0;
                });
            });
        });
    }
}

// ----- Coverage colormap & image conversion ------------------------------

/// Map a normalized scalar `t ∈ [0, 1]` to a viridis-like RGBA colour.
fn viridis_like(t: f32) -> [u8; 4] {
    // 5 control points sampled along the viridis colormap.
    const STOPS: [[f32; 3]; 5] = [
        [68.0, 1.0, 84.0],
        [59.0, 82.0, 139.0],
        [33.0, 145.0, 140.0],
        [94.0, 201.0, 98.0],
        [253.0, 231.0, 37.0],
    ];
    let n = STOPS.len() - 1;
    let f = (t.clamp(0.0, 1.0) * n as f32).min(n as f32);
    let i = (f as usize).min(n - 1);
    let u = f - i as f32;
    let a = STOPS[i];
    let b = STOPS[i + 1];
    [
        (a[0] * (1.0 - u) + b[0] * u) as u8,
        (a[1] * (1.0 - u) + b[1] * u) as u8,
        (a[2] * (1.0 - u) + b[2] * u) as u8,
        255,
    ]
}

/// Render a [`CoverageMap`] as an egui `ColorImage`. Finite pixels are mapped
/// through a viridis-like gradient over `[0, t_max]`; pixels never covered are
/// drawn as dark grey.
fn coverage_to_color_image(map: &CoverageMap) -> egui::ColorImage {
    let mut t_max: f32 = 0.0;
    for v in &map.data {
        if v.is_finite() && *v > t_max {
            t_max = *v;
        }
    }
    let inv = if t_max > 0.0 { 1.0 / t_max } else { 1.0 };

    let mut bytes = Vec::with_capacity(map.width * map.height * 4);
    for v in &map.data {
        let rgba = if v.is_finite() {
            viridis_like(*v * inv)
        } else {
            [25, 25, 28, 255]
        };
        bytes.extend_from_slice(&rgba);
    }
    egui::ColorImage::from_rgba_unmultiplied([map.width, map.height], &bytes)
}
