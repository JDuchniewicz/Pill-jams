use pill_engine::{define_global_component, game::*};

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

define_global_component!(GameState {
    bombs: Vec<Bomb>,
    water_points: Vec<WaterPoint>,
});

const INFLUENCE_RADIUS: f32 = 0.1;
const MAX_VELOCITY: f32 = 200.0;
const KICK_STRENGTH: f32 = 150.0;
pub const TIME_TO_DETONATE: f32 = 1.0;

/// World-space span of the normalized water-point X coordinates (0..1).
/// Shared with the visual mesh so world-space bomb positions map to the right water point.
pub(crate) const WATER_X_MIN: f32 = -100.0;
pub(crate) const WATER_X_MAX: f32 = 100.0;

/// Number of simulated water points spanning the normalized X range 0..1.
const WATER_POINT_COUNT: usize = 128;

/// Creates the water simulation state and registers its systems.
/// Must run before `visuals::start`, which builds the water mesh from `GameState`.
pub fn start(engine: &mut Engine) -> Result<()> {
    engine.add_global_component(GameState {
        bombs: Vec::new(),
        water_points: (0..WATER_POINT_COUNT)
            .map(|i| WaterPoint {
                x: i as f32 / (WATER_POINT_COUNT - 1) as f32,
                y: 0.0,
                velocity: 0.0,
            })
            .collect(),
    })?;

    engine.add_system("evolve_waves", evolve_waves)?;
    engine.add_system("update_bombs", update_bombs)?;

    Ok(())
}

fn evolve_waves(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
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

/// Queues a bomb at a world-space position. The X coordinate is converted to the
/// normalized 0..1 range used by the simulated water points.
pub fn add_bomb(engine: &mut Engine, x: f32, y: f32) -> Result<()> {
    let normalized_x =
        ((x - WATER_X_MIN) / (WATER_X_MAX - WATER_X_MIN)).clamp(0.0, 1.0);

    let game_state = engine.get_global_component_mut::<GameState>()?;

    game_state.bombs.push(Bomb {
        x: normalized_x,
        y,
        time_to_detonate: TIME_TO_DETONATE,
    });

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
        game_state.bombs.retain(|bomb| bomb.time_to_detonate > 0.0);

        detonated_bombs
    };

    // Kick the water points based on the detonated bombs
    for (x, y) in detonated_bombs {
        kick_water(engine, x, y, KICK_STRENGTH)?;
    }

    Ok(())
}
