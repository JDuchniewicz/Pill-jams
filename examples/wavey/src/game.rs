use pill_engine::{define_component, define_global_component, game::*};

use std::sync::{Arc, Mutex};

// TODO: for now we are playing locally -> maybe also play over network
const NUM_PLAYERS: u8 = 2;
const MOVEMENT_SPEED: f32 = 2.0;
const DZ: f32 = 0.1;

pub struct WaterPoint {
    pub x: f32,
    pub y: f32,
    pub velocity: f32,
}
pub struct Bomb {
    pub x: f32,
    pub y: f32,
    pub time_to_detonate: f32,
}

define_component!(PlayerComponent { id: PlayerId });

define_global_component!(GameState{
   bombs: Vec<Bomb>,
   water_points: Vec<WaterPoint>,
});
const INFLUENCE_RADIUS: f32 = 0.1;
const MAX_VELOCITY: f32 = 5.0;
const KICK_STRENGTH: f32 = 1.0;

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
        engine.register_component::<AudioListenerComponent>(active_scene)?;
        engine.register_component::<AudioSourceComponent>(active_scene)?;
        engine.register_component::<PlayerComponent>(active_scene)?;

        // Add game systems
        //engine.add_system("rotation_system", rotation_system)?;
        engine.add_system("evolve_waves", evolve_waves)?;
        engine.add_system("player_movement_system", player_movement_system)?;
        engine.add_system("playa_system", playa_system)?;
        // --- Visuals setup ---
        crate::visuals::start(engine, active_scene)?;
        // Add render passes
        engine.add_render_pass(Box::new(CartoonPass::new()))?;

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
        engine.add_global_component(GameState {
            bombs: Vec::new(),
            last_bomb: 0,
            water_points: (0..128)
                .map(|i| WaterPoint {
                    x: i as f32 / 127.0,
                    y: 0.0,
                    velocity: 0.0,
                })
                .collect(),
        })?;

        Ok(())
    }
}

// --- Systems ---

fn rotation_system(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
    #[cfg(debug_assertions)]
    println!("Delta time: {}", delta_time);

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

fn player_movement_system(engine: &mut Engine) -> Result<()> {
    let input = engine.get_global_component::<InputComponent>()?;

    // movements
    let movements: [(f32, f32); NUM_PLAYERS as usize] = {
        std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            let l_stick_y = input.get_gamepad_axis(id, GamepadAxis::LeftStickY);
            let l_stick_x = input.get_gamepad_axis(id, GamepadAxis::LeftStickX);
            (l_stick_x, l_stick_y)
        })
    };

    // wobbles
    let wobbles: [(f32, f32); NUM_PLAYERS as usize] = {
        std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            let r_stick_y = input.get_gamepad_axis(id, GamepadAxis::RightStickY);
            let r_stick_x = input.get_gamepad_axis(id, GamepadAxis::RightStickX);
            (r_stick_x, r_stick_y)
        })
    };

    // actions

    // TODO: debug - this just moves one player
    //let (k_w,k_s,k_a,k_d)=(
    //    input.get_key(KeyboardKey::KeyW),
    //    input.get_key(KeyboardKey::KeyS),
    //    input.get_key(KeyboardKey::KeyA),
    //    input.get_key(KeyboardKey::KeyD));

    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;

    for (_, transform_component, player_component) in
        engine.iterate_two_components_mut::<TransformComponent, PlayerComponent>()?
    {
        let movement = movements[player_component.id as usize];

        // TODO: check which is which
        if movement.0.abs() > DZ {
            transform_component
                .translate(movement.0 * MOVEMENT_SPEED * delta_time, Direction::Right);
        }
    }

    Ok(())
}
fn kick_water(engine: &mut Engine, x: f32, _y: f32, strength: f32) -> Result<()> {
    let game_state = engine.get_global_component_mut::<GameState>()?;
    let points = &mut game_state.water_points;
    let n = points.len();

    for i in 1..n - 1 {
        let point = &mut points[i];
        let distance = (point.x - x).abs();

        if distance < INFLUENCE_RADIUS {
            // Linear falloff: full strength at the hit point, 0 at the radius
            let influence = 1.0 - distance / INFLUENCE_RADIUS;
            let velocity = strength * influence;
            point.velocity = (point.velocity + velocity).clamp(-MAX_VELOCITY, MAX_VELOCITY);
        }
    }

    Ok(())
}
fn add_bomb(engine: &mut Engine) -> Result<()> {
    let game_state = engine.get_global_component_mut::<GameState>()?;
    const DEBUG_X: f32 = 0.5;
    const DEBUG_Y: f32 = 0.0;

    let bomb = Bomb {
        x: DEBUG_X,
        y: DEBUG_Y,
        time_to_detonate: 5.0,
    };

    game_state.bombs.push(bomb);

    Ok(())
}
fn update_bombs(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;

    // Scope the GameState borrow so it ends before kick_water borrows the engine again
    let detonated_bombs: Vec<(f32, f32)> = {
        let game_state = engine.get_global_component_mut::<GameState>()?;

        for bomb in &mut game_state.bombs {
            bomb.time_to_detonate -= delta_time;
        }

        let detonated_bombs: Vec<(f32, f32)> = game_state
            .bombs
            .iter()
            .filter(|bomb| bomb.time_to_detonate <= 0.0)
            .map(|bomb| (bomb.x, bomb.y))
            .collect();

        // Remove bombs that have detonated
        game_state
            .bombs
            .retain(|bomb| bomb.time_to_detonate > 0.0);

        detonated_bombs
    };

    // Kick the water points based on the detonated bombs
    for (x, y) in detonated_bombs {
        kick_water(engine, x, y, KICK_STRENGTH)?;
    }

    Ok(())
}
fn playa_system(engine: &mut Engine) -> Result<()> {
    let space_pressed = engine
        .get_global_component::<InputComponent>()?
        .get_key_pressed(KeyboardKey::Space);

    if space_pressed {
        add_bomb(engine)?;
    }

    update_bombs(engine)?;

    Ok(())
}