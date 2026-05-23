use crate::constellation::{self, Constellation, SimulationInput};

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
        }
    }
}

impl App {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

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

            ui.horizontal(|ui| {
                ui.label("Analytical Solution");
                match constellation.max_revisit_time() {
                    Some(seconds) => ui.label(format!(
                        "Maximum revisit time: {:.3} hours ({:.3} days)",
                        seconds / 3600.0,
                        seconds / 86400.0,
                    )),
                    None => ui.label("Maximum revisit time: N/A (invalid geometry or parameters)"),
                };
            });

            ui.label("Simulation");
            // plot the constellation

            ui.separator();

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
