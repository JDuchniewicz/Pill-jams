use pill_engine::{define_global_component, game::*};

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

const INFLUENCE_RADIUS: f32 = 0.1;
const KICK_STRENGTH: f32 = 150.0;
pub const TIME_TO_DETONATE: f32 = 1.0;

/// Random delay range between swells, in seconds.
const SWELL_INTERVAL_MIN: f32 = 1.0;
const SWELL_INTERVAL_MAX: f32 = 3.0;

/// Delay between waves generated at the pool's edge, in seconds.
const WAVE_POOL_INTERVAL: f32 = 1.0;

/// World-space span of the normalized water-point X coordinates (0..1).
/// Shared with the visual mesh, which maps 0..1 onto this span.
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
                vertical_velocity_delta: 0.0,
            })
            .collect(),
        time_until_swell: rand::random_range(SWELL_INTERVAL_MIN..SWELL_INTERVAL_MAX),
        time_until_wave_pool: WAVE_POOL_INTERVAL,
    })?;

    engine.add_system("evolve_waves", evolve_waves)?;
    engine.add_system("update_bombs", update_bombs)?;
    engine.add_system("swell_system", swell_system)?;
    engine.add_system("wave_pool_system", wave_pool_system)?;

    Ok(())
}
fn wave_pool_system(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;

    // Scope the GameState borrow so it ends before `swell` borrows the engine again.
    let should_swell = {
        let game_state = engine.get_global_component_mut::<GameState>()?;
        game_state.time_until_wave_pool -= delta_time;

        if game_state.time_until_wave_pool <= 0.0 {
            // Schedule the next wave; without this the timer stays negative and
            // a wave is injected every frame, making the water rise forever.
            game_state.time_until_wave_pool = WAVE_POOL_INTERVAL;
            true
        } else {
            false
        }
    };

    if should_swell {
        // A swell rolls in from a random end of the water.
        let direction1 = -1.0;
        let direction2 = 1.0;
        swell(engine, direction1, direction2)?;
    }

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
    // Gravity-like pull back to the rest level. With free (Neumann) ends there is no
    // fixed anchor, so without this a uniform vertical offset never returns to 0.
    const RESTORING_FORCE: f32 = 1.0;
    // How quickly (per second) the average level is pulled back to y = 0.
    const BASELINE_RESTORE_RATE: f32 = 2.0;

    let dx = points[1].x - points[0].x;
    if dx <= 0.0 {
        return Ok(());
    }

    // World-space spacing between neighbouring points (used to turn the surface slope into a
    // world-space horizontal velocity).
    let dx_world = dx * (WATER_X_MAX - WATER_X_MIN);

    // Explicit integration is only stable while WAVE_SPEED * dt / dx <= 1.
    // Subdivide the frame so every substep stays well inside that bound.
    let max_dt = 0.5 * dx / WAVE_SPEED;
    let steps = (delta_time / max_dt).ceil().max(1.0) as u32;
    let step_dt = delta_time / steps as f32;
    let damping = (1.0 - DAMPING * step_dt).max(0.0);

    let mut accel = vec![0.0f32; n];
    let mut next_y = vec![0.0f32; n];

    // First-order Mur absorbing boundary coefficient. An outgoing wave satisfies
    // y_t = ±c * y_x at the ends, which maps to this discrete update.
    let mur = (WAVE_SPEED * step_dt - dx) / (WAVE_SPEED * step_dt + dx);

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
                WAVE_SPEED * WAVE_SPEED * laplacian / (dx * dx) - RESTORING_FORCE * points[i].y;
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
                WAVE_SPEED * WAVE_SPEED * laplacian / (dx * dx) - RESTORING_FORCE * next_y[i];

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
    let recentre = (BASELINE_RESTORE_RATE * delta_time).min(1.0);
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
    let game_state = engine.get_global_component_mut::<GameState>()?;
    let normalized_x = (x - WATER_X_MIN) / (WATER_X_MAX - WATER_X_MIN);
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
fn swell_system(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;

    // Scope the GameState borrow so it ends before `swell` borrows the engine again.
    let should_swell = {
        let game_state = engine.get_global_component_mut::<GameState>()?;
        game_state.time_until_swell -= delta_time;

        if game_state.time_until_swell <= 0.0 {
            // Schedule the next swell at a fresh random time.
            game_state.time_until_swell =
                rand::random_range(SWELL_INTERVAL_MIN..SWELL_INTERVAL_MAX);
            true
        } else {
            false
        }
    };

    if should_swell {
        // A swell rolls in from a random end of the water.
        let direction1 = rand::random_range(-0.9..0.9);
        let direction2 = rand::random_range(-0.1..0.1) + direction1;
        swell(engine, direction1, direction2)?;
    }

    Ok(())
}

fn swell(engine: &mut Engine, direction: f32, direction2: f32) -> Result<()> {
    let strength = rand::random_range(10.0..15.0);
    let strength2: f32 = rand::random_range(10.0..15.);

    // Water points are normalized to 0..1; a swell enters from one end.

    kick_water(engine, direction, 0.0, strength)?;
    kick_water(engine, direction2, 0.0, strength2)?;
    Ok(())
}

/// Interpolated water surface height at a world-space X position.
pub fn water_height_at_x(engine: &Engine, world_x: f32) -> Result<f32> {
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

    // Average the points within a window around x (about 10 world units wide).
    let mut sum = 0.0;
    let mut count = 0;
    for point in points {
        if (point.x - x).abs() <= 0.05 {
            sum += point.y;
            count += 1;
            if count >= 10 {
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
