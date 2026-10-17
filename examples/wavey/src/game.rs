use crate::cartoon_pass::{CartoonComponent, CartoonPass};
use crate::uv_pass::UvQuadPass;
use pill_engine::{define_global_component, game::*};

use std::sync::{Arc, Mutex};

// Orthographic size shared with the egui overlay (updated by `camera_zoom_system`).
// `Arc<Mutex<_>>` allows the UI callback (which only receives `&egui::Context`) to read it.
define_global_component!(GameUIStateComponent {
    ortho_size: Arc<Mutex<f32>>,
    draw_sprite_bounds: Arc<Mutex<bool>>,
    draw_uv_quad: Arc<Mutex<bool>>,
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
        // Fullscreen UV quad: a background pass, runs before the opaque PBR pass
        engine.add_background_pass(Box::new(UvQuadPass::new()))?;
        // --- Camera scale UI ---

        let ortho_size = Arc::new(Mutex::new(200.0_f32));
        let draw_sprite_bounds = Arc::new(Mutex::new(false));
        let draw_uv_quad = Arc::new(Mutex::new(true));
        engine.add_global_component(GameUIStateComponent {
            ortho_size: Arc::clone(&ortho_size),
            draw_sprite_bounds: Arc::clone(&draw_sprite_bounds),
            draw_uv_quad: Arc::clone(&draw_uv_quad),
        })?;
        engine.set_game_overlay(move |ctx| {
            let ortho_size = *ortho_size
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut draw_sprite_bounds = draw_sprite_bounds
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut draw_uv_quad = draw_uv_quad
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            egui::Window::new("Wavey")
                .resizable(false)
                .default_pos(egui::pos2(10.0, 160.0))
                .show(ctx, |ui| {
                    ui.label(format!("Ortho size: {:.1}", ortho_size));
                    ui.checkbox(&mut draw_sprite_bounds, "Sprite bounds");
                    ui.checkbox(&mut draw_uv_quad, "UV rainbow quad");
                });
        })?;

        Ok(())
    }
}
