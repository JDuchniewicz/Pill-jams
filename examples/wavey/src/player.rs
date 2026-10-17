use pill_engine::{define_component, game::*};

use crate::water::add_bomb;
use crate::water::water_height_at_x;
const BUOYANCY_STRENGTH: f32 = 100.0;
const SHIP_MASS: f32 = 1.0;
define_component!(PlayerComponent { id: PlayerId });

// TODO: for now we are playing locally -> maybe also play over network
const NUM_PLAYERS: u8 = 2;
const THRUST_FORCE: f32 = 25.0;
const DZ: f32 = 0.1;

pub fn start(engine: &mut Engine, active_scene: SceneHandle) -> Result<()> {
    engine.register_component::<RigidBodyComponent>(active_scene)?;
    engine.register_component::<ColliderComponent>(active_scene)?;
    engine.register_component::<PlayerComponent>(active_scene)?;

    engine.add_system("player_movement_system", player_movement_system)?;

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
                .build(),
        )
        .with_component(
            SpriteRenderingComponent::builder()
                .texture(&ship_texture_handle)
                .size(Vector2f::new(20.0, 20.0))
                // 2D game objects stay visible over the water and the other player
                .depth_test(false)
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
                .shape(SharedShape::cuboid(0.5, 0.8, 0.5))
                .friction(0.4)
                .restitution(0.0)
                .mass(1.0)
                .build(),
        )
        .with_component(PlayerComponent {
            id: PlayerId::Player1,
        })
        .build();

    // TODO: re-enable wwhen buyonacy
    let physics = engine.get_global_component_mut::<PhysicsWorldComponent>()?;
   // physics.set_gravity(Vector3f::new(0.0, 0.0, 0.0));

    Ok(())
}

fn player_movement_system(engine: &mut Engine) -> Result<()> {
    // Gather input first: the immutable `InputComponent` borrow must end before `add_bomb`,
    // which borrows `GameState` mutably.
    let (movements, _wobbles, drop_bombs) = {
        let input = engine.get_global_component::<InputComponent>()?;

        // movements
        let movements: [(f32, f32); NUM_PLAYERS as usize] = std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            let l_stick_y = input.get_gamepad_axis(id, GamepadAxis::LeftStickY);
            let l_stick_x = input.get_gamepad_axis(id, GamepadAxis::LeftStickX);
            (l_stick_x, l_stick_y)
        });

        // wobbles
        let wobbles: [(f32, f32); NUM_PLAYERS as usize] = std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            let r_stick_y = input.get_gamepad_axis(id, GamepadAxis::RightStickY);
            let r_stick_x = input.get_gamepad_axis(id, GamepadAxis::RightStickX);
            (r_stick_x, r_stick_y)
        });

        // actions: drop a bomb
        let drop_bombs: [bool; NUM_PLAYERS as usize] = std::array::from_fn(|i| {
            let id = PlayerId::try_from(i as u8).unwrap();
            input.get_gamepad_button_pressed(id, GamepadButton::A)
                || input.get_key_pressed(KeyboardKey::Space)
        });

        (movements, wobbles, drop_bombs)
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

            let translation = body.translation();
            ship_states.push((
                handle,
                input_x,
                Vector3f::new(translation.x, translation.y, translation.z),
            ));
        }
    }

    // Compute buoyancy from the current water height
    let mut ship_forces = Vec::with_capacity(ship_states.len());
    for (handle, input_x, player_position) in ship_states {
        let water_height = water_height_at_x(engine, player_position.x)?;

        let buoyancy_force = if player_position.y < water_height {
            let depth = water_height - player_position.y;
            Vector3f::new(0.0, depth * BUOYANCY_STRENGTH, 0.0)
        } else {
            Vector3f::new(0.0, 0.0, 0.0)
        };

        ship_forces.push((handle, input_x, buoyancy_force));
    }
    // Apply thrust and buoyancy to Rapier's bodies
    let physics = engine.get_global_component_mut::<PhysicsWorldComponent>()?;

    for (handle, input_x, buoyancy_force) in ship_forces {
        let Some(body) = physics.rigid_body_set.get_mut(handle) else {
            continue;
        };

        body.reset_forces(true);

        // Apply thrust along X
        if input_x.abs() > DZ {
            body.add_force(Vector3f::new(-input_x * THRUST_FORCE, 0.0, 0.0), true);
        }

        // Buoyancy pushes up while the player is below the water surface
        if buoyancy_force.y != 0.0 {
            body.add_force(buoyancy_force, true);
        }
        //constant downward force due to ship weight
        body.add_force(Vector3f::new(0.0, -SHIP_MASS * BUOYANCY_STRENGTH, 0.0), true);
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
