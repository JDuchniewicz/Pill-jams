use crate::background_pass::{BackgroundQuadPass, SkyParams};
use crate::cartoon_pass::{CartoonComponent, CartoonPass};
use pill_engine::{define_global_component, game::*};

use std::sync::{Arc, Mutex};

// Orthographic size shared with the egui overlay (updated by `camera_zoom_system`).
// `Arc<Mutex<_>>` allows the UI callback (which only receives `&egui::Context`) to read it.
define_global_component!(GameUIStateComponent {
    ortho_size: Arc<Mutex<f32>>,
    draw_sprite_bounds: Arc<Mutex<bool>>,
    draw_background: Arc<Mutex<bool>>,
    sky: Arc<Mutex<SkyParams>>,
});

pub struct Game {}

impl PillGame for Game {
    fn start(&self, engine: &mut Engine) -> Result<()> {
        // --- Basic setup ---

        // Create scene
        let active_scene = engine.create_scene("default")?;
        engine.set_active_scene(active_scene)?;

        // Register graphics components
        engine.register_component::<TransformComponent>(active_scene)?;
        engine.register_component::<MeshRenderingComponent>(active_scene)?;
        engine.register_component::<SpriteRenderingComponent>(active_scene)?;
        engine.register_component::<CartoonComponent>(active_scene)?;
        engine.register_component::<CameraComponent>(active_scene)?;

        // Register game-specific components
        engine.register_component::<AudioListenerComponent>(active_scene)?;
        engine.register_component::<AudioSourceComponent>(active_scene)?;

        // --- Water simulation (must exist before visuals builds the water mesh) ---
        crate::water::start(engine)?;

        // --- Visuals setup ---
        crate::visuals::start(engine, active_scene)?;
        crate::player::start(engine, active_scene)?;

        // Add render passes
        engine.add_render_pass(Box::new(CartoonPass::new()))?;
        // 2D sky & water gradient: a background pass, runs before the opaque PBR pass
        engine.add_background_pass(Box::new(BackgroundQuadPass::new()))?;
        // --- Camera scale UI ---

        let ortho_size = Arc::new(Mutex::new(200.0_f32));
        let draw_sprite_bounds = Arc::new(Mutex::new(false));
        let draw_background = Arc::new(Mutex::new(true));
        let sky = Arc::new(Mutex::new(SkyParams::default()));
        engine.add_global_component(GameUIStateComponent {
            ortho_size: Arc::clone(&ortho_size),
            draw_sprite_bounds: Arc::clone(&draw_sprite_bounds),
            draw_background: Arc::clone(&draw_background),
            sky: Arc::clone(&sky),
        })?;
        engine.set_game_overlay(move |ctx| {
            let ortho_size = *ortho_size
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut draw_sprite_bounds = draw_sprite_bounds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut draw_background = draw_background
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut sky = sky.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            egui::Window::new("Wavey")
                .resizable(true)
                .default_width(260.0)
                .default_pos(egui::pos2(10.0, 160.0))
                .show(ctx, |ui| {
                    ui.label(format!("Ortho size: {:.1}", ortho_size));
                    ui.checkbox(&mut draw_sprite_bounds, "Sprite bounds");
                    ui.checkbox(&mut draw_background, "Sky & water background");

                    ui.separator();
                    egui::CollapsingHeader::new("Sky & water")
                        .default_open(true)
                        .show(ui, |ui| {
                            color_row(ui, "Sky top", &mut sky.sky_top);
                            color_row(ui, "Sky horizon", &mut sky.sky_horizon);
                            color_row(ui, "Water horizon", &mut sky.water_horizon);
                            color_row(ui, "Water bottom", &mut sky.water_bottom);

                            ui.add(
                                egui::Slider::new(&mut sky.horizon, 0.0..=1.0).text("Water line"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.horizon_softness, 0.0..=0.2)
                                    .text("Line softness"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.wave_amplitude, 0.0..=0.05)
                                    .text("Wave amplitude"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.wave_frequency, 0.0..=100.0)
                                    .text("Wave frequency"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.wave_speed, 0.0..=20.0)
                                    .text("Wave speed"),
                            );
                        });

                    egui::CollapsingHeader::new("Sun")
                        .default_open(false)
                        .show(ui, |ui| {
                            color_row(ui, "Glow color", &mut sky.sun_color);
                            ui.add(
                                egui::Slider::new(&mut sky.sun_intensity, 0.0..=2.0)
                                    .text("Intensity"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.sun_position[0], 0.0..=1.0)
                                    .text("Sun X"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.sun_position[1], 0.0..=1.0)
                                    .text("Sun Y"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.sun_radius, 0.001..=1.0).text("Radius"),
                            );
                            ui.add(
                                egui::Slider::new(&mut sky.sun_softness, 0.0..=1.0)
                                    .text("Softness"),
                            );
                        });
                });
        })?;

        Ok(())
    }
}

/// One labelled colour picker row.
fn color_row(ui: &mut egui::Ui, label: &str, color: &mut [f32; 3]) {
    ui.horizontal(|ui| {
        ui.color_edit_button_rgb(color);
        ui.label(label);
    });
}
