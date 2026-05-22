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
            mu: 3.986004418E14,
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
                ui.label("Write something: ");
                ui.text_edit_singleline(&mut self.label);
            });

            ui.horizontal(|ui| {
                ui.label("inclination [°]:")
                    .on_hover_text("orbital inclincation in degrees");
                ui.add(
                    egui::DragValue::new(&mut self.inclination)
                        .speed(0.5)
                        .range(0.0..=90.0),
                );
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
                ui.label("altitude [km]: ")
                    .on_hover_text("Height above the planets surface");
                ui.add(egui::DragValue::new(&mut self.altitude).speed(0.5));
                ui.label("µ: ")
                    .on_hover_text("Standard Gravitational Parameter [m³/s²]");
                ui.add(egui::DragValue::new(&mut self.mu).speed(1.0));
                ui.label("ω: ")
                    .on_hover_text("Rotation Speed of Planet in degrees per hour [°/h]");
                ui.add(egui::DragValue::new(&mut self.omega).speed(1.0));

                let mut selected_planet: Option<(&str, f32, f32, f32)> = None;
                ui.menu_button(egui::RichText::new("🌍"), |ui| {
                    if ui.button("Mercury").clicked() {
                        selected_planet = Some(("Mercury", 22032.0, 6.138e-7, 2439.7));
                        ui.close();
                    }
                    if ui.button("Venus").clicked() {
                        selected_planet = Some(("Venus", 324859.0, -2.992e-7, 6051.8));
                        ui.close();
                    }
                    if ui.button("Earth").clicked() {
                        selected_planet = Some(("Earth", 398600.4418, 360.0 / 24.0, 6378.1));
                        ui.close();
                    }
                    if ui.button("Mars").clicked() {
                        selected_planet = Some(("Mars", 42828.0, 7.088e-5, 3389.5));
                        ui.close();
                    }
                    if ui.button("Jupiter").clicked() {
                        selected_planet = Some(("Jupiter", 126686534.0, 1.75853e-4, 69911.0));
                        ui.close();
                    }
                    if ui.button("Saturn").clicked() {
                        selected_planet = Some(("Saturn", 37931187.0, 1.6379e-4, 58232.0));
                        ui.close();
                    }
                    if ui.button("Uranus").clicked() {
                        selected_planet = Some(("Uranus", 5793939.0, -1.012e-4, 25362.0));
                        ui.close();
                    }
                    if ui.button("Neptune").clicked() {
                        selected_planet = Some(("Neptune", 6836529.0, 1.083e-4, 24622.0));
                        ui.close();
                    }
                });

                if let Some((_planet, mu, omega, radius)) = selected_planet {
                    self.mu = mu;
                    self.omega = omega;
                    self.radius = radius;
                }
            });

            ui.separator();

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Maximum revisit time: 1.0 days");
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
