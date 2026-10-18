use pill_engine::{define_component, define_global_component, game::*};

use crate::water::add_bomb;
use crate::water::water_height_at_x;

use std::sync::{Arc, Mutex};

define_component!(PlayerComponent { id: PlayerId });

// Live-tunable ship physics parameters, shared between the physics system and the
// "Player physics" egui window.
define_global_component!(PlayerPhysicsComponent {
    params: Arc<Mutex<PlayerPhysicsParams>>,
});

// TODO: for now we are playing locally -> maybe also play over network
const NUM_PLAYERS: u8 = 2;

const METERS_PER_WORLD_UNIT: f32 = 0.20;
const GRAVITY_MPS2: f32 = -9.81;
const BUOYANCY_STRENGTH: f32 = 100.0 / METERS_PER_WORLD_UNIT;

// Pixel coordinates of the actual white hull on the 374x374 ship_test.png.
// The sprite continues to render at 20x20 world units, unchanged.
const SHIP_ART_SIZE: f32 = 374.0;
const SHIP_SPRITE_SIZE: f32 = 20.0;
const HULL_ART_HALF_WIDTH: f32 = (345.0 - 33.0) * SHIP_SPRITE_SIZE / (2.0 * SHIP_ART_SIZE);
const HULL_ART_HALF_HEIGHT: f32 = (169.0 - 111.0) * SHIP_SPRITE_SIZE / (2.0 * SHIP_ART_SIZE);
const DECK_LOCAL_Y: f32 = (0.5 - 111.0 / SHIP_ART_SIZE) * SHIP_SPRITE_SIZE;

// Widen the contact region slightly beyond the white hull for forgiving impacts.
// SpriteRenderer still draws the ORIGINAL 20x20 texture; these affect physics only.
const HULL_CONTACT_MARGIN_X: f32 = 1.08;
const HULL_CONTACT_MARGIN_Y: f32 = 1.18;

// Keep the total mass at `ship_mass`, but concentrate most of it near the ship
// origin (below the painted hull), rather than the centroid of the raised shape.
// This is physical ballast, not a fake restoring torque.
const HULL_GEOMETRY_MASS_FRACTION: f32 = 0.15;

/// Every tunable of the ship physics, live-edited from the "Player physics" egui window.
#[derive(Clone, Debug)]
pub struct PlayerPhysicsParams {
    /// Trapezoidal hull half-extents, in sprite-local world units.
    /// The deck stays aligned with the original sprite as hull height changes.
    pub hull_half_width: f32,
    pub hull_half_height: f32,
    pub hull_half_depth: f32,
    /// Mass applied to the hull collider.
    pub ship_mass: f32,
    /// Hull collision surface properties.
    pub friction: f32,
    pub restitution: f32,
    /// Rigid body damping and gravity scaling.
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub gravity_scale: f32,
    pub ccd_enabled: bool,
    /// Thrust applied along X and the stick dead zone below which no thrust is applied.
    pub thrust_force: f32,
    pub input_deadzone: f32,
    /// Upward force per world unit of hull submersion depth.
    pub buoyancy_strength: f32,
    /// Sideways force that makes the ship slide down the water slope.
    pub wave_slope_strength: f32,
    /// The water slope is clamped to ±this before it becomes a force.
    pub slope_clamp: f32,
    /// Outer sample X as a fraction of the physical hull half-width (0..1).
    pub hull_sample_x: f32,
    /// Local height ABOVE the hull's physical bottom for the outer/center samples.
    pub hull_sample_y: f32,
    pub hull_center_y: f32,
    //Drag coefficients for the water
    //Linear drag force
    pub water_drag_linear: f32, // F = c1*v
    //Quadratic drag force
    pub water_drag_quadratic: f32, // F = c2*v^2
}

impl Default for PlayerPhysicsParams {
    fn default() -> Self {
        Self {
            hull_half_width: HULL_ART_HALF_WIDTH * HULL_CONTACT_MARGIN_X,
            hull_half_height: HULL_ART_HALF_HEIGHT * HULL_CONTACT_MARGIN_Y,
            hull_half_depth: 0.3,
            ship_mass: 10.0,
            friction: 0.4,
            restitution: 0.0,
            linear_damping: 5.0,
            angular_damping: 3.0,
            gravity_scale: 1.0,
            ccd_enabled: true,
            thrust_force: 750.0,
            input_deadzone: 0.1,
            buoyancy_strength: BUOYANCY_STRENGTH,
            wave_slope_strength: 100.0,
            slope_clamp: 1.0,
            hull_sample_x: 0.8,
            hull_sample_y: 0.22,
            hull_center_y: 0.25,
            water_drag_linear: 10.0,
            water_drag_quadratic: 10.0,
        }
    }
}

/// Two 3D convex prisms: the painted trapezoidal hull and the wheelhouse.
/// Coordinates are in LOCAL 20x20 sprite space, not pixel space.
/// Shape is slightly larger than the white art for robust contact handling.
fn ship_hull_shape(params: &PlayerPhysicsParams) -> SharedShape {
    let world_x = |px: f32| {
        (px / SHIP_ART_SIZE - 0.5)
            * SHIP_SPRITE_SIZE
            * (params.hull_half_width / HULL_ART_HALF_WIDTH)
    };
    let world_y = |py: f32| (0.5 - py / SHIP_ART_SIZE) * SHIP_SPRITE_SIZE;

    let deck_y = DECK_LOCAL_Y;
    let bottom_y = deck_y - 2.0 * params.hull_half_height;
    let depth = params.hull_half_depth;

    // Hull: (33,111)-(345,111)-(326,169)-(67,169) in the original PNG.
    // Its top stays on the sprite deck; the enlarged thickness goes downwards.
    let hull_outline = [
        (world_x(33.0), deck_y),
        (world_x(345.0), deck_y),
        (world_x(326.0), bottom_y),
        (world_x(67.0), bottom_y),
    ];
    let mut hull_vertices = Vec::with_capacity(8);
    for z in [-depth, depth] {
        for (x, y) in hull_outline {
            hull_vertices.push(Vector3f::new(x, y, z));
        }
    }
    let hull = SharedShape::convex_hull(&hull_vertices).expect("valid ship hull");

    // Wheelhouse: separate from the deck, so there is no artificial solid wall
    // connecting its roof to the hull. Also gives the cabin real collisions.
    let cabin_outline = [
        (world_x(268.0), world_y(40.0)),
        (world_x(342.0), world_y(40.0)),
        (world_x(342.0), world_y(91.0)),
        (world_x(268.0), world_y(91.0)),
    ];
    let mut cabin_vertices = Vec::with_capacity(8);
    for z in [-depth, depth] {
        for (x, y) in cabin_outline {
            cabin_vertices.push(Vector3f::new(x, y, z));
        }
    }
    let cabin = SharedShape::convex_hull(&cabin_vertices).expect("valid ship cabin");

    SharedShape::compound(vec![
        (Default::default(), hull),
        (Default::default(), cabin),
    ])
}

pub fn start(engine: &mut Engine, active_scene: SceneHandle) -> Result<()> {
    engine.register_component::<RigidBodyComponent>(active_scene)?;
    engine.register_component::<ColliderComponent>(active_scene)?;
    engine.register_component::<PlayerComponent>(active_scene)?;

    engine.add_system("player_movement_system", player_movement_system)?;
    engine.add_system("submerge_system", submerge_system)?;

    // Shared, live-editable physics parameters (also read by the "Player physics" egui window).
    let player_physics = Arc::new(Mutex::new(PlayerPhysicsParams::default()));
    engine.add_global_component(PlayerPhysicsComponent {
        params: Arc::clone(&player_physics),
    })?;
    let physics = player_physics
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();

    let ship_texture_handle = engine.add_resource::<Texture>(Texture::new(
        "ship_test",
        TextureType::Color,
        ResourceLoader::Path("textures/ship_test.png".into()),
    ))?;

    // Player 1 is a camera-facing sprite; the transform drives its world position.
    engine
        .build_entity(active_scene)
        .with_component(
            TransformComponent::builder()
                .position(Vector3f::new(-1.0, 0.0, 0.0))
                .rotation(Vector3f::new(0.0, 180.0, 0.0))
                .build(),
        )
        .with_component(
            SpriteRenderingComponent::builder()
                .texture(&ship_texture_handle)
                .size(Vector2f::new(20.0, 20.0))
                // 2D game objects stay visible over the water and the other player
                .depth_test(false)
                // Hull below the water surface is drawn at half brightness
                .submerge(0.5, 0.0)
                .build(),
        )
        .with_component(
            RigidBodyComponent::builder()
                .body_type(RigidBodyType::Dynamic)
                .linear_damping(physics.linear_damping)
                .angular_damping(physics.angular_damping)
                .locked_axes(
                    LockedAxes::TRANSLATION_LOCKED_Z
                        | LockedAxes::ROTATION_LOCKED_X
                        | LockedAxes::ROTATION_LOCKED_Y,
                )
                .ccd_enabled(physics.ccd_enabled)
                .build(),
        )
        .with_component(
            ColliderComponent::builder()
                .shape(ship_hull_shape(&physics))
                .friction(physics.friction)
                .restitution(physics.restitution)
                .mass(physics.ship_mass * HULL_GEOMETRY_MASS_FRACTION)
                .build(),
        )
        .with_component(PlayerComponent {
            id: PlayerId::Player1,
        })
        .build();

    // Player 2 is a camera-facing sprite; the transform drives its world position.
    engine
        .build_entity(active_scene)
        .with_component(
            TransformComponent::builder()
                .position(Vector3f::new(-20.0, 0.0, 0.0))
                .build(),
        )
        .with_component(
            SpriteRenderingComponent::builder()
                .texture(&ship_texture_handle)
                .size(Vector2f::new(20.0, 20.0))
                // 2D game objects stay visible over the water and the other player
                .depth_test(false)
                // Hull below the water surface is drawn at half brightness
                .submerge(0.5, 0.0)
                .build(),
        )
        .with_component(
            RigidBodyComponent::builder()
                .body_type(RigidBodyType::Dynamic)
                .linear_damping(physics.linear_damping)
                .angular_damping(physics.angular_damping)
                .locked_axes(
                    LockedAxes::TRANSLATION_LOCKED_Z
                        | LockedAxes::ROTATION_LOCKED_X
                        | LockedAxes::ROTATION_LOCKED_Y,
                )
                .ccd_enabled(physics.ccd_enabled)
                .build(),
        )
        .with_component(
            ColliderComponent::builder()
                .shape(ship_hull_shape(&physics))
                .friction(physics.friction)
                .restitution(physics.restitution)
                .mass(physics.ship_mass * HULL_GEOMETRY_MASS_FRACTION)
                .build(),
        )
        .with_component(PlayerComponent {
            id: PlayerId::Player2,
        })
        .build();

    engine
        .get_global_component_mut::<PhysicsWorldComponent>()?
        .set_gravity(Vector3f::new(
            0.0,
            GRAVITY_MPS2 / METERS_PER_WORLD_UNIT,
            0.0,
        ));

    Ok(())
}

fn player_movement_system(engine: &mut Engine) -> Result<()> {
    // Copy the shared params out so no lock is held while the physics world is borrowed.
    let params = {
        let shared = engine
            .get_global_component::<PlayerPhysicsComponent>()?
            .params
            .clone();
        let guard = shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    };

    // Use actual lower hull positions, not the old tiny samples around (0,-0.2).
    // Wide spacing provides restoring torque when one side dips into a wave.
    let sample_x = params.hull_half_width * params.hull_sample_x.clamp(0.0, 0.95);
    let hull_bottom_y = DECK_LOCAL_Y - 2.0 * params.hull_half_height;
    let hull_points: [Vector3f; 3] = [
        Vector3f::new(-sample_x, hull_bottom_y + params.hull_sample_y, 0.0),
        Vector3f::new(0.0, hull_bottom_y + params.hull_center_y, 0.0),
        Vector3f::new(sample_x, hull_bottom_y + params.hull_sample_y, 0.0),
    ];
    // Gather input first: the immutable `InputComponent` borrow must end before `add_bomb`,
    // which borrows `GameState` mutably.
    let (movements, keyboard_movements, drop_bombs) = {
        let input = engine.get_global_component::<InputComponent>()?;

        // movements
        let movements: [(f32, f32); NUM_PLAYERS as usize] = std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            let l_stick_y = input.get_gamepad_axis(id, GamepadAxis::LeftStickY);
            let l_stick_x = input.get_gamepad_axis(id, GamepadAxis::LeftStickX);
            (l_stick_x, l_stick_y)
        });
        let keyboard_movements: [(f32, f32); NUM_PLAYERS as usize] = std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            let mut x = 0.0;
            let mut y = 0.0;
            if input.get_key(KeyboardKey::KeyA) {
                x -= 1.0;
            }
            if input.get_key(KeyboardKey::KeyD) {
                x += 1.0;
            }
            (x, y)
        });

        // actions: drop a bomb
        let drop_bombs: [bool; NUM_PLAYERS as usize] = std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            input.get_gamepad_button_pressed(id, GamepadButton::A)
                || input.get_key_pressed(KeyboardKey::Space)
        });

        (movements, keyboard_movements, drop_bombs)
    };

    // Collect player rigid body handles and input values
    let mut ships = Vec::new();

    for (_, rb, player) in
        engine.iterate_two_components_mut::<RigidBodyComponent, PlayerComponent>()?
    {
        if let Some(handle) = rb.get_rigid_body_handle() {
            let input_x =
                movements[player.id as usize].0 + keyboard_movements[player.id as usize].0;
            ships.push((handle, input_x));
        }
    }

    // Read body positions first. This borrow of the physics world must end before
    // querying the water surface through the engine.
    let mut ship_states = Vec::new();
    {
        let physics = engine.get_global_component::<PhysicsWorldComponent>()?;

        for (handle, input_x) in ships {
            let Some(body) = physics.rigid_body_set.get(handle) else {
                continue;
            };

            ship_states.push((handle, input_x, body.translation(), body.rotation()));
        }
    }

    // Compute buoyancy from the current water height
    let mut ship_forces = Vec::with_capacity(ship_states.len());
    for (handle, input_x, player_position, player_rotation) in ship_states {
        // store water samples at points
        let mut water_samples = [(0.0f32, 0.0f32); 3];
        // calculate the force triplet
        let mut point_buoyancy_forces = Vec::with_capacity(3);
        for (i, local_point) in hull_points.iter().enumerate() {
            let world_point = player_position + player_rotation * local_point;
            water_samples[i] = (world_point.x, water_height_at_x(engine, world_point.x)?);

            let point_buoyancy_force = if world_point.y < water_samples[i].1 {
                let depth = water_samples[i].1 - world_point.y;
                Vector3f::new(
                    0.0,
                    depth * params.buoyancy_strength / hull_points.len() as f32,
                    0.0,
                )
            } else {
                Vector3f::new(0.0, 0.0, 0.0)
            };
            point_buoyancy_forces.push((world_point, point_buoyancy_force));
        }

        let dx = water_samples[2].0 - water_samples[0].0;
        let slope = if dx.abs() > 0.001 {
            (water_samples[2].1 - water_samples[0].1) / dx
        } else {
            0.0
        };
        ship_forces.push((handle, input_x, point_buoyancy_forces, slope));
    }
    // Apply thrust and buoyancy to Rapier's bodies.
    // Construct the expected compound collider once, not every frame per ship.
    let expected_hull_shape = ship_hull_shape(&params);
    let expected_aabb = expected_hull_shape.compute_local_aabb();
    let physics = engine.get_global_component_mut::<PhysicsWorldComponent>()?;

    for (handle, input_x, point_buoyancy_forces, slope) in ship_forces {
        let Some(body) = physics.rigid_body_set.get_mut(handle) else {
            continue;
        };

        // Keep the *total* mass at params.ship_mass: only 15% lives in the
        // collision geometry high above the origin. The rest is ballast at
        // body origin, below the hull. Rapier handles the resulting COM.
        // Build MASS PROPERTIES at the body's local origin, NOT just an
        // "additional mass" value. `set_additional_mass()` alone would keep
        // the COM at the hull's raised centroid (and still cause capsizing).
        // This low, wide ballast has inertia but no extra collision geometry.
        let mut ballast_props =
            SharedShape::cuboid(params.hull_half_width * 0.7, 0.15, params.hull_half_depth)
                .mass_properties(1.0);
        ballast_props.set_mass(params.ship_mass * (1.0 - HULL_GEOMETRY_MASS_FRACTION), true);
        body.set_additional_mass_properties(ballast_props, true);

        if (body.angular_damping() - params.angular_damping).abs() > 1.0e-6 {
            body.set_angular_damping(params.angular_damping);
        }
        if (body.gravity_scale() - params.gravity_scale).abs() > 1.0e-6 {
            body.set_gravity_scale(params.gravity_scale, true);
        }
        if body.is_ccd_enabled() != params.ccd_enabled {
            body.enable_ccd(params.ccd_enabled);
        }

        // Update only the ship's compound collider. Compare local AABB bounds,
        // not the rotated world AABB, so rocking never triggers a rebuild.
        let collider_handles = body.colliders().to_vec();
        for collider_handle in collider_handles {
            let Some(collider) = physics.collider_set.get_mut(collider_handle) else {
                continue;
            };

            let aabb = collider.shape().compute_local_aabb();
            let eps = 1.0e-4;
            let shape_changed = collider.shape().as_compound().is_none()
                || (aabb.mins - expected_aabb.mins).length() > eps
                || (aabb.maxs - expected_aabb.maxs).length() > eps;

            if shape_changed {
                collider.set_shape(expected_hull_shape.clone());
            }
            if (collider.friction() - params.friction).abs() > 1.0e-6 {
                collider.set_friction(params.friction);
            }
            if (collider.restitution() - params.restitution).abs() > 1.0e-6 {
                collider.set_restitution(params.restitution);
            }
            let hull_mass = params.ship_mass * HULL_GEOMETRY_MASS_FRACTION;
            if (collider.mass() - hull_mass).abs() > 1.0e-6 {
                collider.set_mass(hull_mass);
            }
        }

        // Make ballast / collider mass-property changes effective *before* the
        // buoyancy forces are applied at world points in this update.
        body.recompute_mass_properties_from_colliders(&physics.collider_set);

        body.reset_forces(true);
        body.reset_torques(true);

        let in_water = point_buoyancy_forces.iter().any(|(_, force)| force.y > 0.0);
        // Apply thrust along X
        if input_x.abs() > params.input_deadzone && in_water {
            body.add_force(
                Vector3f::new(-input_x * params.thrust_force, 0.0, 0.0),
                true,
            );
        }

        let submerged_points = point_buoyancy_forces
            .iter()
            .filter(|(_, force)| force.y > 0.0)
            .count();
        let submersion = submerged_points as f32 / point_buoyancy_forces.len() as f32;
        // Buoyancy pushes up while the player is below the water surface
        for (world_point, buoyancy_force) in point_buoyancy_forces {
            if buoyancy_force.y != 0.0 {
                body.add_force_at_point(buoyancy_force, world_point, true);
            }
        }
        //Water drag: apply a linear+quadratic drag force based on the velocity of the ship in water
        if submersion > 0.0 {
            let velocity = body.linvel();
            let speed = velocity.length();
            if speed > 0.0 {
                let drag_coefficient =
                    params.water_drag_linear + params.water_drag_quadratic * speed;
                let drag_force = velocity * -drag_coefficient * submersion;
                body.add_force(drag_force, true);
            }
        }

        // Add the force of going up or down the slope
        if in_water {
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

    let mut bomb_positions: Vec<(f32, f32)> = Vec::new();
    for (_, transform_component, player_component) in
        engine.iterate_two_components_mut::<TransformComponent, PlayerComponent>()?
    {
        if drop_bombs[player_component.id as usize] {
            //        add_bomb(engine, transform_component.position.x, transform_component.position.y)?;
            bomb_positions.push((
                transform_component.position.x,
                transform_component.position.y,
            ));
        }
    }

    for (x, y) in bomb_positions {
        add_bomb(engine, x, y)?;
    }

    Ok(())
}

/// Keeps each sprite's waterline in sync with the procedural water surface, so the part of the
/// sprite below the mesh's top edge is drawn at `submerge_factor` brightness.
fn submerge_system(engine: &mut Engine) -> Result<()> {
    // Sampling the water surface borrows the engine immutably, so collect positions first.
    let mut sample_points: Vec<(EntityHandle, f32)> = Vec::new();
    for (entity, transform, sprite) in
        engine.iterate_two_components_mut::<TransformComponent, SpriteRenderingComponent>()?
    {
        // Sprites without a submerge effect (factor 1.0) do not need a waterline
        if sprite.submerge_factor < 1.0 {
            sample_points.push((entity, transform.position.x));
        }
    }

    let mut waterlines: Vec<(EntityHandle, f32)> = Vec::with_capacity(sample_points.len());
    for (entity, world_x) in sample_points {
        waterlines.push((entity, water_height_at_x(engine, world_x)?));
    }

    if waterlines.is_empty() {
        return Ok(());
    }

    for (entity, _, sprite) in
        engine.iterate_two_components_mut::<TransformComponent, SpriteRenderingComponent>()?
    {
        if let Some((_, height)) = waterlines.iter().find(|(handle, _)| *handle == entity) {
            sprite.set_submerge_height(*height);
        }
    }

    Ok(())
}
