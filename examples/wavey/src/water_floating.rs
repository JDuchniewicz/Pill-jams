use pill_engine::{define_component, game::*};

use crate::player::{PlayerPhysicsComponent, PlayerPhysicsParams};
use crate::water::water_height_at_x;

// Per-entity water-floating setup, plus the water state measured for it every frame.
//
// Any entity with this component (and a `RigidBodyComponent`) gets buoyancy at its hull probe
// points, submersion-scaled water drag and the wave-slope force from `water_floating_system`.
// The ships and the ducks share the same code path; only the probe points differ per entity.
define_component!(WaterFloatingComponent {
    // Local-space buoyancy probes (hull samples), in world units.
    hull_points: [Vector3f; 3],
    // --- Filled by `water_floating_system` each frame ---
    // True while at least one probe is below the water surface.
    in_water: bool,
    // Fraction of probes below the water (0..1); scales the water drag.
    submersion: f32,
    // Water surface height at the entity's X position.
    water_level: f32,
});

impl WaterFloatingComponent {
    pub fn new(hull_points: [Vector3f; 3]) -> Self {
        Self {
            hull_points,
            in_water: false,
            submersion: 0.0,
            water_level: 0.0,
        }
    }

    /// Height of a world-space Y above the water surface (positive = above the water).
    pub fn height_above_water(&self, world_y: f32) -> f32 {
        world_y - self.water_level
    }

    /// True when `world_y` is more than `depth` world units below the water surface — the same
    /// comparison the particle opacity band uses (`height_above_water` < -0.2).
    pub fn is_below_water(&self, world_y: f32, depth: f32) -> bool {
        self.height_above_water(world_y) < -depth.max(0.0)
    }

    /// True when every hull probe is below the surface, i.e. the whole hull is submerged.
    pub fn is_fully_submerged(&self) -> bool {
        self.submersion >= 0.999
    }
}

/// Copies the live-tunable floating parameters out of the shared player physics global.
fn read_params(engine: &Engine) -> Result<PlayerPhysicsParams> {
    let shared = engine.get_global_component::<PlayerPhysicsComponent>()?;
    let guard = shared
        .params
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(guard.clone())
}

/// Buoyancy, water drag and wave-slope force for every `WaterFloatingComponent` entity.
///
/// This runs before `player_movement_system`: it clears last frame's forces, applies the water
/// forces and publishes `in_water` / `water_level` so the input systems can read them the same
/// frame, then the player system adds thrust on top.
fn water_floating_system(engine: &mut Engine) -> Result<()> {
    let params = read_params(engine)?;

    // Collect the floating entities and their rigid body handles.
    let mut entities = Vec::new();
    for (entity, rb, floating) in
        engine.iterate_two_components_mut::<RigidBodyComponent, WaterFloatingComponent>()?
    {
        if let Some(handle) = rb.get_rigid_body_handle() {
            entities.push((entity, handle, floating.hull_points));
        }
    }
    if entities.is_empty() {
        return Ok(());
    }

    // Read the body transforms; this physics borrow must end before sampling the water.
    let mut states = Vec::with_capacity(entities.len());
    {
        let physics = engine.get_global_component::<PhysicsWorldComponent>()?;
        for (entity, handle, hull_points) in entities {
            let Some(body) = physics.rigid_body_set.get(handle) else {
                continue;
            };
            states.push((
                entity,
                handle,
                hull_points,
                body.translation(),
                body.rotation(),
            ));
        }
    }

    // Sample the water surface and build the per-probe buoyancy forces.
    let mut floating_states = Vec::with_capacity(states.len());
    for (entity, handle, hull_points, position, rotation) in states {
        let mut water_samples = [(0.0f32, 0.0f32); 3];
        let mut point_forces = [(Vector3f::ZERO, Vector3f::ZERO); 3];
        for (i, local_point) in hull_points.iter().enumerate() {
            let world_point = position + rotation * local_point;
            water_samples[i] = (world_point.x, water_height_at_x(engine, world_point.x)?);

            let point_buoyancy_force = if world_point.y < water_samples[i].1 {
                let depth = water_samples[i].1 - world_point.y;
                Vector3f::new(
                    0.0,
                    depth * params.buoyancy_strength / hull_points.len() as f32,
                    0.0,
                )
            } else {
                Vector3f::ZERO
            };
            point_forces[i] = (world_point, point_buoyancy_force);
        }

        let dx = water_samples[2].0 - water_samples[0].0;
        let slope = if dx.abs() > 0.001 {
            (water_samples[2].1 - water_samples[0].1) / dx
        } else {
            0.0
        };
        let water_level = water_samples[1].1;
        let submerged_points = point_forces
            .iter()
            .filter(|(_, force)| force.y > 0.0)
            .count();
        floating_states.push((
            entity,
            handle,
            point_forces,
            slope,
            water_level,
            submerged_points > 0,
            submerged_points as f32 / point_forces.len() as f32,
        ));
    }

    // Apply the water forces to Rapier's bodies.
    {
        let physics = engine.get_global_component_mut::<PhysicsWorldComponent>()?;

        for (_, handle, point_forces, slope, _, in_water, submersion) in &floating_states {
            let Some(body) = physics.rigid_body_set.get_mut(*handle) else {
                continue;
            };

            // Forces accumulate until reset, so clear the previous frame's before re-adding.
            body.reset_forces(true);
            body.reset_torques(true);

            // Buoyancy pushes up while the probe is below the water surface.
            for &(world_point, buoyancy_force) in point_forces {
                if buoyancy_force.y != 0.0 {
                    body.add_force_at_point(buoyancy_force, world_point, true);
                }
            }

            // Water drag: linear + quadratic, scaled by how much of the hull is submerged.
            if *submersion > 0.0 {
                let velocity = body.linvel();
                let speed = velocity.length();
                if speed > 0.0 {
                    let drag_coefficient =
                        params.water_drag_linear + params.water_drag_quadratic * speed;
                    body.add_force(velocity * -drag_coefficient * *submersion, true);
                }
            }

            // Slope force: slide down the water surface.
            if *in_water {
                body.add_force(
                    Vector3f::new(
                        -slope.clamp(-params.slope_clamp, params.slope_clamp)
                            * params.wave_slope_strength,
                        0.0,
                        0.0,
                    ),
                    true,
                );
            }
        }
    }

    // Publish the measured water state back to the components.
    for (entity, _, floating) in
        engine.iterate_two_components_mut::<RigidBodyComponent, WaterFloatingComponent>()?
    {
        if let Some((_, _, _, _, water_level, in_water, submersion)) =
            floating_states.iter().find(|state| state.0 == entity)
        {
            floating.water_level = *water_level;
            floating.in_water = *in_water;
            floating.submersion = *submersion;
        }
    }

    Ok(())
}

/// Registers the component and the floating system. Call before building floating entities.
pub fn start(engine: &mut Engine, active_scene: SceneHandle) -> Result<()> {
    engine.register_component::<WaterFloatingComponent>(active_scene)?;
    engine.add_system("water_floating_system", water_floating_system)?;
    Ok(())
}
