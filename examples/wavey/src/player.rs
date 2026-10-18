use pill_engine::{define_component, game::*};

use crate::water::add_bomb;
use crate::water::water_height_at_x;
const BUOYANCY_STRENGTH: f32 = 100.0;
const SHIP_MASS: f32 = 10.0;
const WAVE_SLOPE_STRENGTH: f32 = 100.0;
define_component!(PlayerComponent { id: PlayerId });

// TODO: for now we are playing locally -> maybe also play over network
const NUM_PLAYERS: u8 = 2;
const THRUST_FORCE: f32 = 750.0;
const DZ: f32 = 0.1;

pub fn start(engine: &mut Engine, active_scene: SceneHandle) -> Result<()> {
    engine.register_component::<RigidBodyComponent>(active_scene)?;
    engine.register_component::<ColliderComponent>(active_scene)?;
    engine.register_component::<PlayerComponent>(active_scene)?;

    engine.add_system("player_movement_system", player_movement_system)?;
    engine.add_system("submerge_system", submerge_system)?;

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
                .linear_damping(5.0)
                .angular_damping(3.0)
                .locked_axes(
                    LockedAxes::TRANSLATION_LOCKED_Z
                        | LockedAxes::ROTATION_LOCKED_X
                        | LockedAxes::ROTATION_LOCKED_Y,
                )
                .ccd_enabled(true)
                .build(),
        )
        .with_component(
            ColliderComponent::builder()
                .shape(SharedShape::cuboid(1.0, 0.25, 0.3))
                .friction(0.4)
                .restitution(0.0)
                .mass(SHIP_MASS)
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
                .linear_damping(5.0)
                .angular_damping(3.0)
                .locked_axes(
                    LockedAxes::TRANSLATION_LOCKED_Z
                        | LockedAxes::ROTATION_LOCKED_X
                        | LockedAxes::ROTATION_LOCKED_Y,
                )
                .ccd_enabled(true)
                .build(),
        )
        .with_component(
            ColliderComponent::builder()
                .shape(SharedShape::cuboid(1.0, 0.25, 0.3))
                .friction(0.4)
                .restitution(0.0)
                .mass(SHIP_MASS)
                .build(),
        )
        .with_component(PlayerComponent {
            id: PlayerId::Player2,
        })
        .build();

    Ok(())
}

fn player_movement_system(engine: &mut Engine) -> Result<()> {
    const HULL_POINTS: [Vector3f; 3] = [
        Vector3f::new(-0.8, -0.22, 0.0),
        Vector3f::new(0.0, -0.25, 0.0),
        Vector3f::new(0.8, -0.22, 0.0),
    ];
    // Gather input first: the immutable `InputComponent` borrow must end before `add_bomb`,
    // which borrows `GameState` mutably.
    let (movements, drop_bombs) = {
        let input = engine.get_global_component::<InputComponent>()?;

        // movements
        let movements: [(f32, f32); NUM_PLAYERS as usize] = std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            let l_stick_y = input.get_gamepad_axis(id, GamepadAxis::LeftStickY);
            let l_stick_x = input.get_gamepad_axis(id, GamepadAxis::LeftStickX);
            (l_stick_x, l_stick_y)
        });

        // actions: drop a bomb
        let drop_bombs: [bool; NUM_PLAYERS as usize] = std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            input.get_gamepad_button_pressed(id, GamepadButton::A)
                || input.get_key_pressed(KeyboardKey::Space)
        });

        (movements, drop_bombs)
    };

    // Collect player rigid body handles and input values
    let mut ships = Vec::new();

    for (_, rb, player) in
        engine.iterate_two_components_mut::<RigidBodyComponent, PlayerComponent>()?
    {
        if let Some(handle) = rb.get_rigid_body_handle() {
            let input_x = movements[player.id as usize].0;
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
        for (i, local_point) in HULL_POINTS.iter().enumerate() {
            let world_point = player_position + player_rotation * local_point;
            water_samples[i] = (world_point.x, water_height_at_x(engine, world_point.x)?);

            let point_buoyancy_force = if world_point.y < water_samples[i].1 {
                let depth = water_samples[i].1 - world_point.y;
                Vector3f::new(0.0, depth * BUOYANCY_STRENGTH, 0.0)
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
    // Apply thrust and buoyancy to Rapier's bodies
    let physics = engine.get_global_component_mut::<PhysicsWorldComponent>()?;

    for (handle, input_x, point_buoyancy_forces, slope) in ship_forces {
        let Some(body) = physics.rigid_body_set.get_mut(handle) else {
            continue;
        };

        body.reset_forces(true);
        body.reset_torques(true);

        // Apply thrust along X
        if input_x.abs() > DZ {
            body.add_force(Vector3f::new(-input_x * THRUST_FORCE, 0.0, 0.0), true);
        }

        let in_water = point_buoyancy_forces.iter().any(|(_, force)| force.y > 0.0);

        // Buoyancy pushes up while the player is below the water surface
        for (world_point, buoyancy_force) in point_buoyancy_forces {
            if buoyancy_force.y != 0.0 {
                body.add_force_at_point(buoyancy_force, world_point, true);
            }
        }

        // Add the force of going up or down the slope
        if in_water {
            body.add_force(
                Vector3f::new(-slope.clamp(-1.0, 1.0) * WAVE_SLOPE_STRENGTH, 0.0, 0.0),
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
