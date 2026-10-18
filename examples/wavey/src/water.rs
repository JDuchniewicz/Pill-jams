use pill_engine::{define_global_component, game::*};

use std::sync::{Arc, Mutex};

pub struct WaterPoint {
    pub x: f32,
    pub y: f32,
    pub velocity: f32,
    pub vertical_velocity_delta: f32,
}

pub struct Bomb {
    pub x: f32,
    pub y: f32,
    pub time_to_detonate: f32,
}

define_global_component!(GameState {
    bombs: Vec<Bomb>,
    water_points: Vec<WaterPoint>,
    // Seconds left until the next random swell.
    time_until_swell: f32,
    time_until_wave_pool : f32
});

// Live-tunable water simulation parameters, shared between the simulation systems and the
// "Water simulation" egui section.
define_global_component!(WaterParamsComponent {
    params: Arc<Mutex<WaterParams>>,
});

/// Every tunable of the water simulation, live-edited from egui.
#[derive(Clone, Debug)]
pub struct WaterParams {
    // --- Wave equation ---
    /// Propagation speed of the wave equation.
    pub wave_speed: f32,
    /// Velocity damping per second.
    pub damping: f32,
    /// Pull of every point back toward y = 0.
    pub restoring_force: f32,
    /// How quickly (per second) the average level is recentred on y = 0.
    pub baseline_restore_rate: f32,
    // --- Kicks / bombs ---
    /// Impulse added to the water by a bomb.
    pub kick_strength: f32,
    /// Normalized-X radius a kick affects.
    pub influence_radius: f32,
    /// Bomb fuse, in seconds.
    pub time_to_detonate: f32,
    // --- Random swells ---
    /// Minimum/maximum delay between random swells, in seconds.
    pub swell_interval_min: f32,
    pub swell_interval_max: f32,
    /// Minimum/maximum strength of a swell kick.
    pub swell_strength_min: f32,
    pub swell_strength_max: f32,
    /// Spread of the main swell entry point (± around the middle).
    pub swell_direction_spread: f32,
    /// Extra offset for the second swell kick.
    pub swell_direction_jitter: f32,
    // --- Edge waves ---
    /// Delay between waves generated at the pool's edges, in seconds.
    pub wave_pool_interval: f32,
    // --- Surface queries ---
    /// Normalized-X window averaged when sampling the surface height.
    pub height_sample_window: f32,
    /// Maximum number of points averaged by a height query.
    pub height_sample_limit: usize,
}

impl Default for WaterParams {
    fn default() -> Self {
        Self {
            wave_speed: 1.0,
            damping: 0.5,
            restoring_force: 1.0,
            baseline_restore_rate: 2.0,
            kick_strength: 150.0,
            influence_radius: 0.1,
            time_to_detonate: 1.0,
            swell_interval_min: 1.0,
            swell_interval_max: 3.0,
            swell_strength_min: 10.0,
            swell_strength_max: 15.0,
            swell_direction_spread: 0.9,
            swell_direction_jitter: 0.1,
            wave_pool_interval: 1.0,
            height_sample_window: 0.05,
            height_sample_limit: 10,
        }
    }
}

/// Snapshot of the shared water params (the lock is released immediately).
fn water_params(engine: &Engine) -> Result<WaterParams> {
    let shared = engine
        .get_global_component::<WaterParamsComponent>()?
        .params
        .clone();
    let guard = shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(guard.clone())
}

/// World-space span of the normalized water-point X coordinates (0..1).
/// Shared with the visual mesh, which maps 0..1 onto this span.
pub(crate) const WATER_X_MIN: f32 = -100.0;
pub(crate) const WATER_X_MAX: f32 = 100.0;

/// Number of simulated water points spanning the normalized X range 0..1.
const WATER_POINT_COUNT: usize = 128;

/// Creates the water simulation state and registers its systems.
/// Must run before `visuals::start`, which builds the water mesh from `GameState`.
pub fn start(engine: &mut Engine) -> Result<()> {
    let params = Arc::new(Mutex::new(WaterParams::default()));
    engine.add_global_component(WaterParamsComponent {
        params: Arc::clone(&params),
    })?;

    let defaults = params
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();

    engine.add_global_component(GameState {
        bombs: Vec::new(),
        water_points: (0..WATER_POINT_COUNT)
            .map(|i| WaterPoint {
                x: i as f32 / (WATER_POINT_COUNT - 1) as f32,
                y: 0.0,
                velocity: 0.0,
                vertical_velocity_delta: 0.0,
            })
            .collect(),
        time_until_swell: rand::random_range(
            defaults.swell_interval_min
                ..defaults.swell_interval_max.max(defaults.swell_interval_min),
        ),
        time_until_wave_pool: defaults.wave_pool_interval,
    })?;

    engine.add_system("evolve_waves", evolve_waves)?;
    engine.add_system("update_bombs", update_bombs)?;
    engine.add_system("swell_system", swell_system)?;
    engine.add_system("wave_pool_system", wave_pool_system)?;

    Ok(())
}
fn wave_pool_system(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
    let params = water_params(engine)?;

    // Scope the GameState borrow so it ends before `swell` borrows the engine again.
    let should_swell = {
        let game_state = engine.get_global_component_mut::<GameState>()?;
        game_state.time_until_wave_pool -= delta_time;

        if game_state.time_until_wave_pool <= 0.0 {
            // Schedule the next wave; without this the timer stays negative and
            // a wave is injected every frame, making the water rise forever.
            game_state.time_until_wave_pool = params.wave_pool_interval;
            true
        } else {
            false
        }
    };

    if should_swell {
        // A swell rolls in from a random end of the water.
        let direction1 = -1.0;
        let direction2 = 1.0;
        swell(engine, &params, direction1, direction2)?;
    }

    Ok(())
}

fn evolve_waves(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
    let params = water_params(engine)?;
    let game_state = engine.get_global_component_mut::<GameState>()?;
    let points = &mut game_state.water_points;
    let n = points.len();

    if n < 3 || delta_time <= 0.0 {
        return Ok(());
    }

    // Wave equation tuning
    let wave_speed = params.wave_speed;
    let damping_factor = params.damping;
    // Gravity-like pull back to the rest level. With free (Neumann) ends there is no
    // fixed anchor, so without this a uniform vertical offset never returns to 0.
    let restoring_force = params.restoring_force;
    // How quickly (per second) the average level is pulled back to y = 0.
    let baseline_restore_rate = params.baseline_restore_rate;

    let dx = points[1].x - points[0].x;
    if dx <= 0.0 {
        return Ok(());
    }

    // Explicit integration is only stable while WAVE_SPEED * dt / dx <= 1.
    // Subdivide the frame so every substep stays well inside that bound.
    let max_dt = 0.5 * dx / wave_speed.max(0.00001);
    let steps = (delta_time / max_dt).ceil().max(1.0) as u32;
    let step_dt = delta_time / steps as f32;
    let damping = (1.0 - damping_factor * step_dt).max(0.0);

    let mut accel = vec![0.0f32; n];
    let mut next_y = vec![0.0f32; n];

    // First-order Mur absorbing boundary coefficient. An outgoing wave satisfies
    // y_t = ±c * y_x at the ends, which maps to this discrete update.
    let mur = (wave_speed * step_dt - dx) / (wave_speed * step_dt + dx);

    for _ in 0..steps {
        // Boundary values at the start of the substep, needed by the Mur update.
        let left_y_old = points[0].y;
        let left_inner_old = points[1].y;
        let right_inner_old = points[n - 2].y;
        let right_y_old = points[n - 1].y;
        let left_v_old = points[0].velocity;
        let right_v_old = points[n - 1].velocity;

        // Acceleration from the current displacements
        for i in 1..n - 1 {
            let laplacian = points[i - 1].y - 2.0 * points[i].y + points[i + 1].y;
            accel[i] =
                wave_speed * wave_speed * laplacian / (dx * dx) - restoring_force * points[i].y;
        }

        // Velocity Verlet, half step 1: advance position with the current velocity
        for i in 1..n - 1 {
            let v_half = points[i].velocity + 0.5 * accel[i] * step_dt;
            points[i].velocity = v_half;
            next_y[i] = points[i].y + v_half * step_dt;
        }

        // Absorbing ends: let outgoing waves cross the boundary instead of
        // reflecting off a fixed edge.
        next_y[0] = left_inner_old + mur * (next_y[1] - left_y_old);
        next_y[n - 1] = right_inner_old + mur * (next_y[n - 2] - right_y_old);

        // Acceleration at the new positions, using the absorbed boundary values
        for i in 1..n - 1 {
            let laplacian = next_y[i - 1] - 2.0 * next_y[i] + next_y[i + 1];
            let acceleration =
                wave_speed * wave_speed * laplacian / (dx * dx) - restoring_force * next_y[i];

            // Velocity Verlet, half step 2: finish the velocity update (with damping)
            points[i].velocity = (points[i].velocity + 0.5 * acceleration * step_dt) * damping;
            points[i].y = next_y[i];
            points[i].vertical_velocity_delta = acceleration * step_dt;
        }

        // The ends follow from the absorbing condition rather than the wave
        // equation, so their velocity is the position change over the substep.
        points[0].velocity = (next_y[0] - left_y_old) / step_dt;
        points[0].y = next_y[0];
        points[0].vertical_velocity_delta = points[0].velocity - left_v_old;

        points[n - 1].velocity = (next_y[n - 1] - right_y_old) / step_dt;
        points[n - 1].y = next_y[n - 1];
        points[n - 1].vertical_velocity_delta = points[n - 1].velocity - right_v_old;
    }

    // The end points are driven by the absorbing condition rather than the
    // restoring force, so repeated forcing can bias the whole surface upward.
    // Gently recentre the pool on its rest level so it always settles near y = 0.
    let mean_y = points.iter().map(|point| point.y).sum::<f32>() / n as f32;
    let recentre = (baseline_restore_rate * delta_time).min(1.0);
    if mean_y != 0.0 {
        for point in points.iter_mut() {
            point.y -= mean_y * recentre;
        }
    }

    // Safety net: never let non-finite values reach the mesh builder.
    if points
        .iter()
        .any(|point| !point.y.is_finite() || !point.velocity.is_finite())
    {
        for point in points.iter_mut() {
            point.y = 0.0;
            point.velocity = 0.0;
        }
    }

    Ok(())
}

fn kick_water(
    engine: &mut Engine,
    x: f32,
    _y: f32,
    strength: f32,
    influence_radius: f32,
) -> Result<()> {
    let game_state = engine.get_global_component_mut::<GameState>()?;
    let points = &mut game_state.water_points;
    let n = points.len();
    let influence_radius = influence_radius.max(0.00001);

    for i in 1..n - 1 {
        let point = &mut points[i];
        let distance = (point.x - x).abs();

        if distance < influence_radius {
            // Linear falloff: full strength at the hit point, 0 at the radius
            let influence = 1.0 - distance / influence_radius;
            let velocity = strength * influence;
            //point.velocity = (point.velocity + velocity).clamp(-MAX_VELOCITY, MAX_VELOCITY);
            let vertical_velocity_delta = velocity;
            point.vertical_velocity_delta =
                ((vertical_velocity_delta).abs() - (point.velocity.abs())).abs();
            point.velocity += velocity;
        }
    }

    Ok(())
}

/// Queues a bomb at a normalized water position (x in 0..1).
pub fn add_bomb(engine: &mut Engine, x: f32, y: f32) -> Result<()> {
    let params = water_params(engine)?;
    let game_state = engine.get_global_component_mut::<GameState>()?;
    let normalized_x = (x - WATER_X_MIN) / (WATER_X_MAX - WATER_X_MIN);
    game_state.bombs.push(Bomb {
        x: normalized_x,
        y,
        time_to_detonate: params.time_to_detonate,
    });

    Ok(())
}

fn update_bombs(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
    let params = water_params(engine)?;

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
        kick_water(engine, x, y, params.kick_strength, params.influence_radius)?;
    }

    Ok(())
}
fn swell_system(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;
    let params = water_params(engine)?;

    // Scope the GameState borrow so it ends before `swell` borrows the engine again.
    let should_swell = {
        let game_state = engine.get_global_component_mut::<GameState>()?;
        game_state.time_until_swell -= delta_time;

        if game_state.time_until_swell <= 0.0 {
            // Schedule the next swell at a fresh random time.
            game_state.time_until_swell = rand::random_range(
                params.swell_interval_min..params.swell_interval_max.max(params.swell_interval_min),
            );
            true
        } else {
            false
        }
    };

    if should_swell {
        // A swell rolls in from a random end of the water.
        let spread = params.swell_direction_spread;
        let jitter = params.swell_direction_jitter;
        let direction1 = rand::random_range(-spread..spread);
        let direction2 = rand::random_range(-jitter..jitter) + direction1;
        swell(engine, &params, direction1, direction2)?;
    }

    Ok(())
}

fn swell(engine: &mut Engine, params: &WaterParams, direction: f32, direction2: f32) -> Result<()> {
    let strength_range = params.swell_strength_min.max(0.0)
        ..params
            .swell_strength_max
            .max(params.swell_strength_min.max(0.0));
    let strength = rand::random_range(strength_range.clone());
    let strength2: f32 = rand::random_range(strength_range);

    // Water points are normalized to 0..1; a swell enters from one end.

    kick_water(engine, direction, 0.0, strength, params.influence_radius)?;
    kick_water(engine, direction2, 0.0, strength2, params.influence_radius)?;
    Ok(())
}

/// Interpolated water surface height at a world-space X position.
pub fn water_height_at_x(engine: &Engine, world_x: f32) -> Result<f32> {
    let params = water_params(engine)?;
    let window = params.height_sample_window.max(0.0);
    let sample_limit = params.height_sample_limit.max(1);

    let game_state = engine.get_global_component::<GameState>()?;
    let points = &game_state.water_points;
    let n = points.len();

    if n < 2 {
        return Ok(0.0);
    }

    // Water points store normalized X (0..1); convert the query from world space.
    let x = ((world_x - WATER_X_MIN) / (WATER_X_MAX - WATER_X_MIN)).clamp(0.0, 1.0);

    if x <= points[0].x {
        return Ok(points[0].y);
    } else if x >= points[n - 1].x {
        return Ok(points[n - 1].y);
    }

    // Average the points within a window around x.
    let mut sum = 0.0;
    let mut count = 0;
    for point in points {
        if (point.x - x).abs() <= window {
            sum += point.y;
            count += 1;
            if count >= sample_limit {
                break;
            }
        }
    }

    if count > 0 {
        Ok(sum / count as f32)
    } else {
        Ok(0.0)
    }
}
