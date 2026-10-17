use pill_engine::{define_component, define_global_component, game::*};

use std::sync::{Arc, Mutex};

define_component!(TagAlphaComponent {});

// Orthographic size shared with the egui overlay (updated by `camera_zoom_system`).
// `Arc<Mutex<_>>` allows the UI callback (which only receives `&egui::Context`) to read it.
define_global_component!(GameUIStateComponent {
    ortho_size: Arc<Mutex<f32>>,
});

pub struct Game {}

impl PillGame for Game {
    fn start(&self, engine: &mut Engine) -> Result<()> {
        // --- Basic setup ---

        // Create scene
        let active_scene = engine.create_scene("default")?;
        engine.set_active_scene(active_scene)?;

        // Register game-specific components
        engine.register_component::<TagAlphaComponent>(active_scene)?;
        engine.register_component::<AudioListenerComponent>(active_scene)?;
        engine.register_component::<AudioSourceComponent>(active_scene)?;

        // Add game systems
        engine.add_system("rotation_system", rotation_system)?;

        // --- Visuals setup ---
        crate::visuals::start(engine, active_scene)?;

        // --- Camera scale UI ---

        let ortho_size = Arc::new(Mutex::new(200.0_f32));
        engine.add_global_component(GameUIStateComponent {
            ortho_size: Arc::clone(&ortho_size),
        })?;
        engine.set_game_overlay(move |ctx| {
            let ortho_size = *ortho_size
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            egui::Window::new("Wavey")
                .resizable(false)
                .default_pos(egui::pos2(10.0, 160.0))
                .show(ctx, |ui| {
                    ui.label(format!("Ortho size: {:.1}", ortho_size));
                });
        })?;

        Ok(())
    }
}

// --- Systems ---

fn rotation_system(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
    //println!("Delta time: {}", delta_time);

    for (_, transform_component, _) in
        engine.iterate_two_components_mut::<TransformComponent, TagAlphaComponent>()?
    {
        transform_component.rotate_around_axis(90.0 * delta_time, Vector3f::new(0.0, 1.0, 0.0));
    }

    Ok(())
}
