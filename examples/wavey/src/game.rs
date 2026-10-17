use pill_engine::{define_component, define_global_component, game::*};

use std::sync::{Arc, Mutex};

pub struct WaterPoint {
    pub x: f32,
    pub y: f32,
    pub velocity: f32,
}

define_component!(TagAlphaComponent {});
define_global_component!(GameState{
   bombs: Vec<i32>,
   last_bomb: i32,
   water_points: Vec<WaterPoint>,
});

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
        engine.add_system("evolve_waves", evolve_waves)?;

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


        // -- initialize the wave component --
        engine
            .add_global_component(
                GameState {
                    bombs: Vec::new(),
                    last_bomb: 0,
                    water_points: (0..128)
                        .map(|i| WaterPoint {
                            x: i as f32 / 127.0,
                            y: 0.0,
                            velocity: 0.0,
                        })
                        .collect(),
                }
            )?;
            
            
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
fn evolve_waves(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
    #[cfg(debug_assertions)]
    println!("Delta time: {}", delta_time);

    let game_state = engine.get_global_component_mut::<GameState>()?;
    let points = &mut game_state.water_points;
    let n = points.len();

    if n < 3 || delta_time <= 0.0 {
        return Ok(());
    }

    // Wave equation tuning
    const WAVE_SPEED: f32 = 1.0;
    const DAMPING: f32 = 0.5;

    let dx = points[1].x - points[0].x;
    if dx <= 0.0 {
        return Ok(());
    }

    // Keep the explicit integration stable: WAVE_SPEED * dt / dx <= 1.
    // Subdivide the frame when delta_time is too large.
    let max_dt = 0.5 * dx / WAVE_SPEED;
    let steps = (delta_time / max_dt).ceil().max(1.0) as u32;
    let step_dt = delta_time / steps as f32;

    let mut velocities = vec![0.0f32; n];

    for _ in 0..steps {
        // Pass 1: compute new velocities from the current displacements
        for i in 1..n - 1 {
            let left = points[i - 1].y;
            let center = points[i].y;
            let right = points[i + 1].y;

            // Second spatial derivative of the displacement
            let laplacian = left - 2.0 * center + right;
            let acceleration = WAVE_SPEED * WAVE_SPEED * laplacian / (dx * dx);

            // Semi-implicit Euler: update velocity, then position
            let mut velocity = points[i].velocity + acceleration * step_dt;
            velocity *= (1.0 - DAMPING * step_dt).max(0.0);

            velocities[i] = velocity;
        }

        // Pass 2: integrate the new velocities into the positions
        for i in 1..n - 1 {
            points[i].velocity = velocities[i];
            points[i].y += velocities[i] * step_dt;
        }

        // Reflective boundaries: ends are fixed
        points[0].velocity = 0.0;
        points[0].y = 0.0;
        points[n - 1].velocity = 0.0;
        points[n - 1].y = 0.0;
    }

    Ok(())
}
