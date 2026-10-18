use pill_engine::{define_component, define_global_component, game::*};

use crate::particle_system::ParticleSystemComponent;
use crate::sounds::schedule_blare;
use crate::sounds::SingleShotAudioComponent;
use crate::water::add_bomb;
use crate::water::water_height_at_x;
use crate::water_floating::WaterFloatingComponent;

use std::sync::{Arc, Mutex};

define_component!(PlayerComponent {
    id: PlayerId,
    last_thrust_direction: Vector3f,
    air_time: f32,
    flip: i32,
    dive_hold_time: f32,
    dive_was_held: bool,
});

// Live-tunable ship physics parameters, shared between the physics system and the
// "Player physics" egui window.
define_global_component!(PlayerPhysicsComponent {
    params: Arc<Mutex<PlayerPhysicsParams>>,
    // Emission point height relative to the water surface, per player (read by the UI).
    emission_height: Arc<Mutex<[f32; NUM_PLAYERS as usize]>>,
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
const AIR_DRAG_DECEL: f32 = 0.5; // deceleration factor when ship is in air

const DIVE_CHARGE_MAX: f32 = 5.0;
const DIVE_CHARGE_MULTIPLIER: f32 = 10.0; // how fast the dive charge builds up
                                          // Downward force applied during the release burst, as a multiple of thrust_force.
                                          // A sustained force is needed: a single impulse is cancelled almost immediately
                                          // by the quadratic water drag.
const DIVE_BURST_THRUST_MULTIPLIER: f32 = 16.0;
// Keep the total mass at `ship_mass`, but concentrate most of it near the ship
// origin (below the painted hull), rather than the centroid of the raised shape.
// This is physical ballast, not a fake restoring torque.
pub(crate) const HULL_GEOMETRY_MASS_FRACTION: f32 = 0.15;

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
    pub thrust_down_force: f32,
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
    /// Emission height relative to the water surface at/below which thrust particles are drawn
    /// at full opacity; below the water they are drawn fainter, and above this height their
    /// opacity fades to zero across `particle_surface_band`.
    pub particle_surface_center: f32,
    /// Height range above `particle_surface_center` over which the particle opacity fades out.
    pub particle_surface_band: f32,
    /// How far below the water the emission point must be before particles turn red/faint. A
    /// floating ship sits slightly below the water, so this offset keeps it in the green
    /// "on the surface" state while normal dives go red.
    pub particle_underwater_depth: f32,
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
            angular_damping: 7.5,
            gravity_scale: 2.0,
            ccd_enabled: true,
            thrust_force: 1250.0,
            thrust_down_force: 750.0,
            input_deadzone: 0.1,
            buoyancy_strength: BUOYANCY_STRENGTH,
            wave_slope_strength: 100.0,
            slope_clamp: 1.0,
            hull_sample_x: 0.8,
            hull_sample_y: 0.22,
            hull_center_y: 0.25,
            // Emit from just above the water line down; the fade hides foam when the ship jumps.
            particle_surface_center: 0.2,
            particle_surface_band: 0.4,
            particle_underwater_depth: 2.5,
            water_drag_linear: 10.0,
            water_drag_quadratic: 0.0,
        }
    }
}

// TODO: hulls are still a bit shite -> can fix them by spawning them further away and enlarging the
// colliders
/// Two 3D convex prisms: the painted trapezoidal hull and the wheelhouse.
/// Coordinates are in LOCAL 20x20 sprite space, not pixel space.
/// Shape is slightly larger than the white art for robust contact handling.
pub(crate) fn ship_hull_shape(params: &PlayerPhysicsParams) -> SharedShape {
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

/// Build a ship with one solid hull and three massless overlap zones.
/// Every collider shares the ship's single Rapier rigid body.
/// The sprite's unflipped direction is +X; mirrored facing is handled
/// when interpreting collision events (not by rotating the rigid body).
fn ship_collider(params: &PlayerPhysicsParams) -> ColliderComponent {
    let third_width = params.hull_half_width / 3.0;
    let section_half_width = third_width + 0.35;
    let section_x = third_width * 2.0;
    let section_y = DECK_LOCAL_Y - params.hull_half_height;

    // Sensors protrude slightly from the hull, so they overlap at contact.
    let section_shape = SharedShape::cuboid(
        section_half_width,
        params.hull_half_height + 0.15,
        params.hull_half_depth + 0.15,
    );

    ColliderComponent::builder()
        .shape(ship_hull_shape(params))
        .friction(params.friction)
        .restitution(params.restitution)
        .mass(params.ship_mass * HULL_GEOMETRY_MASS_FRACTION)
        .part(
            ColliderPart::new("front", section_shape.clone())
                .translation(Vector3f::new(section_x, section_y, 0.0)),
        )
        .part(
            ColliderPart::new("middle", section_shape.clone())
                .translation(Vector3f::new(0.0, section_y, 0.0)),
        )
        .part(
            ColliderPart::new("back", section_shape)
                .translation(Vector3f::new(-section_x, section_y, 0.0)),
        )
        .build()
}

/// Resolve the actual front/back when the sprite is mirrored.
/// Sensor geometry is symmetric, so this does not require physics-body flipping.
fn effective_ship_section(section: &str, facing: i32) -> &str {
    if facing < 0 {
        match section {
            "front" => "back",
            "back" => "front",
            _ => section,
        }
    } else {
        section
    }
}

/// Game systems run before Pill's PostGame physics step, so these are
/// collisions collected in the preceding frame's physics step.
/// React only on Started: a sustained overlap should not retrigger horns.
fn ship_collision_system(engine: &mut Engine) -> Result<()> {
    let collisions = engine
        .get_global_component_mut::<PhysicsWorldComponent>()?
        .drain_collision_events();

    if collisions.is_empty() {
        return Ok(());
    }

    // Snapshot player facing before borrowing components again for sound playback.
    let mut players = Vec::new();
    for (entity, player, _) in
        engine.iterate_two_components_mut::<PlayerComponent, RigidBodyComponent>()?
    {
        players.push((entity, player.flip));
    }

    let mut horns_to_play = Vec::new();
    for collision in collisions {
        if collision.phase != CollisionPhase::Started {
            continue;
        }
        if collision.a.scene != collision.b.scene {
            continue;
        }

        let Some((_, a_flip)) = players
            .iter()
            .find(|(entity, _)| *entity == collision.a.entity)
        else {
            continue;
        };
        let Some((_, b_flip)) = players
            .iter()
            .find(|(entity, _)| *entity == collision.b.entity)
        else {
            continue;
        };

        let part_a = effective_ship_section(&collision.a.part, *a_flip);
        let part_b = effective_ship_section(&collision.b.part, *b_flip);

        // Named sensor vs named sensor. Hull collisions remain physical but
        // are deliberately not counted as zone-to-zone hits.
        let (attacker, victim) = match (part_a, part_b) {
            ("front", "back") => (collision.a.entity, collision.b.entity),
            ("back", "front") => (collision.b.entity, collision.a.entity),
            _ => continue,
        };

        println!("[SHIP HIT] bow of {attacker:?} hit stern of {victim:?}");
        if !horns_to_play.contains(&attacker) {
            horns_to_play.push(attacker);
        }
    }

    if horns_to_play.is_empty() {
        return Ok(());
    }

    // A ship-to-ship hit also queues one of each effect event (subject to their 2 s cooldowns).
    crate::effects::push_collision_effects(engine)?;

    // Use the existing single-shot horn system. Never borrow the ECS audio
    // component while still holding the PhysicsWorldComponent borrow.
    for (entity, _player, audio) in
        engine.iterate_two_components_mut::<PlayerComponent, SingleShotAudioComponent>()?
    {
        if horns_to_play.contains(&entity) {
            schedule_blare(audio)?;
        }
    }

    Ok(())
}

pub fn start(engine: &mut Engine, active_scene: SceneHandle) -> Result<Vec<EntityHandle>> {
    engine.register_component::<RigidBodyComponent>(active_scene)?;
    engine.register_component::<ColliderComponent>(active_scene)?;
    engine.register_component::<PlayerComponent>(active_scene)?;

    // Water floating physics (buoyancy, drag, slope) is shared with ducks. It must be registered
    // before the player system: it clears the per-frame forces and measures the water state that
    // the player system then reads and adds thrust on top of. If it ran after the player system,
    // its force reset would wipe the thrust applied by the player on the same frame.
    crate::water_floating::start(engine, active_scene)?;

    engine.add_system("player_movement_system", player_movement_system)?;
    engine.add_system("submerge_system", submerge_system)?;
    engine.add_system("ship_collision_system", ship_collision_system)?;

    // Particle emitters live on the player entities; the pass draws every emitter in one call.
    crate::particle_system::start(engine, active_scene)?;

    // Shared, live-editable physics parameters (also read by the "Player physics" egui window).
    let player_physics = Arc::new(Mutex::new(PlayerPhysicsParams::default()));
    engine.add_global_component(PlayerPhysicsComponent {
        params: Arc::clone(&player_physics),
        emission_height: Arc::new(Mutex::new([f32::NAN; NUM_PLAYERS as usize])),
    })?;
    let physics = player_physics
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();

    let ship_texture_handle = engine.add_resource::<Texture>(Texture::new(
        "ship_test",
        TextureType::Color,
        ResourceLoader::Path("textures/ship.png".into()),
    ))?;
    let foam_texture_handle = engine.add_resource::<Texture>(Texture::new(
        "foam_particle",
        TextureType::Color,
        ResourceLoader::Path("textures/foam_particle.png".into()),
    ))?;

    let player1 = engine
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
        .with_component(ship_collider(&physics))
        .with_component(PlayerComponent {
            id: PlayerId::Player1,
            flip: 1,
            last_thrust_direction: Vector3f::new(0.0, 0.0, 0.0),
            air_time: 0.0,
            dive_hold_time: 0.0,
            dive_was_held: false,
        })
        .with_component(WaterFloatingComponent::new(ship_buoyancy_points(&physics)))
        .with_component(
            AudioSourceComponent::builder()
                .sound_type(SoundType::Sound2D)
                .volume(0.25)
                .play_on_awake(false)
                .build(),
        )
        .with_component(ParticleSystemComponent::new(foam_texture_handle))
        .build();

    // Player 2 is a camera-facing sprite; the transform drives its world position.
    let player2 = engine
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
        .with_component(ship_collider(&physics))
        .with_component(PlayerComponent {
            id: PlayerId::Player2,
            flip: 1,
            last_thrust_direction: Vector3f::new(0.0, 0.0, 0.0),
            air_time: 0.0,
            dive_hold_time: 0.0,
            dive_was_held: false,
        })
        .with_component(WaterFloatingComponent::new(ship_buoyancy_points(&physics)))
        .with_component(
            AudioSourceComponent::builder()
                .sound_type(SoundType::Sound2D)
                .volume(0.25)
                .play_on_awake(false)
                .build(),
        )
        .with_component(ParticleSystemComponent::new(foam_texture_handle))
        .build();

    engine
        .get_global_component_mut::<PhysicsWorldComponent>()?
        .set_gravity(Vector3f::new(
            0.0,
            GRAVITY_MPS2 / METERS_PER_WORLD_UNIT,
            0.0,
        ));

    Ok(Vec::from([player1, player2]))
}

#[derive(Clone, Copy, Default)]
struct ShipControls {
    movement_x: f32,
    movement_y: f32,
    drop_bomb: bool,
    horn_blare: bool,
}

fn read_ship_controls(engine: &Engine) -> Result<[ShipControls; NUM_PLAYERS as usize]> {
    let input = engine.get_global_component::<InputComponent>()?;
    let keyboard_x =
        (input.get_key(KeyboardKey::KeyD) as i32 - input.get_key(KeyboardKey::KeyA) as i32) as f32;
    let keyboard_y =
        (input.get_key(KeyboardKey::KeyS) as i32 - input.get_key(KeyboardKey::KeyW) as i32) as f32;
    let keyboard_bomb = input.get_key_pressed(KeyboardKey::Space);
    let keyboard_blare = input.get_key_pressed(KeyboardKey::ControlLeft);

    Ok(std::array::from_fn(|i| {
        let id = PlayerId::try_from(i as u8).unwrap();
        ShipControls {
            // Keyboard is reserved for Player 1, not both ships.
            movement_x: input.get_gamepad_axis(id, GamepadAxis::LeftStickX)
                + if i == 0 { keyboard_x } else { 0.0 },
            movement_y: input.get_gamepad_axis(id, GamepadAxis::LeftStickY)
                + if i == 0 { keyboard_y } else { 0.0 },
            drop_bomb: input.get_gamepad_button_pressed(id, GamepadButton::A)
                || (i == 0 && keyboard_bomb),
            horn_blare: input.get_gamepad_button_pressed(id, GamepadButton::Y)
                || (i == 0 && keyboard_blare),
        }
    }))
}

pub(crate) fn ship_buoyancy_points(params: &PlayerPhysicsParams) -> [Vector3f; 3] {
    let sample_x = params.hull_half_width * params.hull_sample_x.clamp(0.0, 0.95);
    let hull_bottom_y = DECK_LOCAL_Y - 2.0 * params.hull_half_height;
    [
        Vector3f::new(-sample_x, hull_bottom_y + params.hull_sample_y, 0.0),
        Vector3f::new(0.0, hull_bottom_y + params.hull_center_y, 0.0),
        Vector3f::new(sample_x, hull_bottom_y + params.hull_sample_y, 0.0),
    ]
}

/// Gives a non-player floating entity (e.g. a duck) the same low ballast as the ships, so it
/// floats upright instead of capsizing around the hull centroid.
pub fn apply_float_ballast(
    engine: &mut Engine,
    entity: EntityHandle,
    params: &PlayerPhysicsParams,
) -> Result<()> {
    let mut body_handle = None;
    for (candidate, _, rb) in
        engine.iterate_two_components_mut::<TransformComponent, RigidBodyComponent>()?
    {
        if candidate == entity {
            body_handle = rb.get_rigid_body_handle();
        }
    }

    let Some(handle) = body_handle else {
        return Ok(());
    };

    let physics = engine.get_global_component_mut::<PhysicsWorldComponent>()?;
    let Some(body) = physics.rigid_body_set.get_mut(handle) else {
        return Ok(());
    };

    let mut ballast_props =
        SharedShape::cuboid(params.hull_half_width * 0.7, 0.15, params.hull_half_depth)
            .mass_properties(1.0);
    ballast_props.set_mass(params.ship_mass * (1.0 - HULL_GEOMETRY_MASS_FRACTION), true);
    body.set_additional_mass_properties(ballast_props, true);
    body.recompute_mass_properties_from_colliders(&physics.collider_set);

    Ok(())
}

/// Return `None` in the low-speed hysteresis band to retain the current facing.
/// Note: thrust uses -input_x, so input sign and travel direction are opposite.
fn desired_facing(velocity_x: f32, input_x: f32, deadzone: f32) -> Option<bool> {
    const MOVING_THRESHOLD: f32 = 0.6;
    const STOPPED_THRESHOLD: f32 = 0.15;

    if velocity_x.abs() >= MOVING_THRESHOLD {
        // Sprite asset is presumed to face right in its unf state.
        Some(velocity_x < 0.0)
    } else if velocity_x.abs() <= STOPPED_THRESHOLD && input_x.abs() > deadzone {
        Some(input_x > 0.0)
    } else {
        None
    }
}

fn player_movement_system(engine: &mut Engine) -> Result<()> {
    // Copy the shared params out so no lock is held while the physics world is borrowed.
    let (params, emission_heights) = {
        let shared = engine.get_global_component::<PlayerPhysicsComponent>()?;
        let params = shared.params.clone();
        let emission_heights = shared.emission_height.clone();
        let guard = params
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (guard.clone(), emission_heights)
    };
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;

    let controls = read_ship_controls(engine)?;

    // Dive charge state, preserved per player across frames.
    let mut next_dive_hold_time = [0.0f32; NUM_PLAYERS as usize];
    let mut next_dive_was_held = [false; NUM_PLAYERS as usize];

    // Collect player rigid body handles, input values and this frame's water state. The floating
    // system runs first, so `in_water` / `water_level` are already up to date.
    let mut ships = Vec::new();

    for (_, rb, player, floating) in engine.iterate_three_components_mut::<
        RigidBodyComponent,
        PlayerComponent,
        WaterFloatingComponent,
    >()? {
        if let Some(handle) = rb.get_rigid_body_handle() {
            let player_index = player.id as usize;
            next_dive_hold_time[player_index] = player.dive_hold_time;
            next_dive_was_held[player_index] = player.dive_was_held;
            ships.push((
                handle,
                player_index,
                controls[player_index].movement_x,
                controls[player_index].movement_y,
                player.air_time,
                player.last_thrust_direction,
                floating.in_water,
                floating.water_level,
            ));
        }
    }

    // New per-player values, written back to `PlayerComponent` after the physics borrow ends.
    let mut next_air_time = [0.0f32; NUM_PLAYERS as usize];
    let mut next_thrust_direction = [Vector3f::ZERO; NUM_PLAYERS as usize];

    // Read body positions first; this physics borrow must end before touching the ECS again.
    let mut ship_states = Vec::new();
    {
        let physics = engine.get_global_component::<PhysicsWorldComponent>()?;

        for (
            handle,
            player_index,
            input_x,
            input_y,
            air_time,
            last_thrust_direction,
            in_water,
            water_level,
        ) in ships
        {
            // Preserve the previous state if this body is missing from the physics world.
            next_air_time[player_index] = air_time;
            next_thrust_direction[player_index] = last_thrust_direction;

            let Some(body) = physics.rigid_body_set.get(handle) else {
                continue;
            };

            ship_states.push((
                handle,
                player_index,
                input_x,
                input_y,
                air_time,
                last_thrust_direction,
                in_water,
                water_level,
                body.translation(),
            ));
        }
    }

    // Emitters publish their emission-point height (spawn point, not the ship origin) below.
    let expected_hull_shape = ship_hull_shape(&params);
    let expected_aabb = expected_hull_shape.compute_local_aabb();
    let mut facing_updates = [None; NUM_PLAYERS as usize];
    // (thrusting, input axis, water level) per player, forwarded to the particle emitters.
    let mut emitter_states = [(false, 0.0f32, 0.0f32); NUM_PLAYERS as usize];
    {
        let physics = engine.get_global_component_mut::<PhysicsWorldComponent>()?;

        for (
            handle,
            player_index,
            input_x,
            input_y,
            mut air_time,
            mut last_thrust_direction,
            in_water,
            water_level,
            _,
        ) in ship_states
        {
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

                // Extra front/middle/back colliders are sensors. They must never
                // receive the compound hull shape, friction, or hull mass.
                if collider.is_sensor() {
                    let third_width = params.hull_half_width / 3.0;
                    let section_half_width = third_width + 0.35;
                    let section_x = third_width * 2.0;
                    let section_y = DECK_LOCAL_Y - params.hull_half_height;

                    let shape = SharedShape::cuboid(
                        section_half_width,
                        params.hull_half_height + 0.15,
                        params.hull_half_depth + 0.15,
                    );
                    let expected_sensor_aabb = shape.compute_local_aabb();
                    let aabb = collider.shape().compute_local_aabb();
                    let eps = 1.0e-4;
                    if (aabb.mins - expected_sensor_aabb.mins).length() > eps
                        || (aabb.maxs - expected_sensor_aabb.maxs).length() > eps
                    {
                        collider.set_shape(shape);
                    }

                    // Front and back are distinguished by the sign of their
                    // existing local X offset; middle remains centered.
                    if let Some(offset) = collider.position_wrt_parent().map(|p| p.translation) {
                        let desired = Vector3f::new(offset.x.signum() * section_x, section_y, 0.0);
                        if (offset - desired).length() > eps {
                            collider.set_translation_wrt_parent(desired);
                        }
                    }
                    continue;
                }

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

            // Make ballast / collider mass-property changes effective before the physics step.
            body.recompute_mass_properties_from_colliders(&physics.collider_set);

            // Decide sprite facing using horizontal velocity, before applying forces.
            facing_updates[player_index] =
                desired_facing(-body.linvel().x, input_x, params.input_deadzone);

            // Apply thrust along X
            // Horizontal thrust needs water contact; diving (S) thrusts regardless.
            let thrusting = (input_x.abs() > params.input_deadzone && in_water)
                || input_y > params.input_deadzone;
            emitter_states[player_index] = (thrusting, input_x, water_level);

            // Horizontal thrust is only available in water. There is no upward
            // thrust: buoyancy alone lifts the ship and still allows jumps.
            let mut thrust = Vector3f::ZERO;
            if in_water && input_x.abs() > params.input_deadzone {
                thrust.x = -input_x;
            }
            // Holding the dive input only charges the dive; it must not move the
            // ship down. On release, the charge becomes a burst duration during
            // which a strong downward force is applied (capped by DIVE_CHARGE_MAX).
            let diving = input_y > params.input_deadzone;
            if diving {
                next_dive_hold_time[player_index] += DIVE_CHARGE_MULTIPLIER * delta_time;
            } else {
                if next_dive_was_held[player_index] {
                    // Convert the accumulated charge into a burst duration.
                    next_dive_hold_time[player_index] =
                        next_dive_hold_time[player_index].min(DIVE_CHARGE_MAX);
                }

                if next_dive_hold_time[player_index] > 0.0 {
                    body.add_force(
                        Vector3f::new(
                            0.0,
                            -params.thrust_down_force * DIVE_BURST_THRUST_MULTIPLIER,
                            0.0,
                        ),
                        true,
                    );
                    next_dive_hold_time[player_index] =
                        (next_dive_hold_time[player_index] - delta_time).max(0.0);
                }
            }
            next_dive_was_held[player_index] = diving;

            if thrust != Vector3f::ZERO {
                body.add_force(thrust * params.thrust_force, true);
                last_thrust_direction = thrust.normalize_or_zero();
            }

            // Airborne: no thrust, just coasting. Drag ramps with the time spent in
            // the air, so the ship keeps its direction but bleeds off speed.
            if in_water {
                air_time = 0.0;
            } else {
                air_time += delta_time;

                let velocity = body.linvel();
                let horizontal_speed = velocity.x.abs();

                if horizontal_speed > 0.0 {
                    // AIR_DRAG_DECEL is a fraction of thrust per second airborne;
                    // convert it to an acceleration using the body's mass.
                    let deceleration =
                        params.thrust_force * AIR_DRAG_DECEL * air_time / body.mass().max(0.001);
                    let delta_v = (deceleration * delta_time).min(horizontal_speed);
                    let new_velocity_x = velocity.x - velocity.x.signum() * delta_v;

                    body.set_linvel(Vector3f::new(new_velocity_x, velocity.y, velocity.z), true);
                }
            }

            next_air_time[player_index] = air_time;
            next_thrust_direction[player_index] = last_thrust_direction;
        }
    } // Release mutable physics-world borrow before accessing ECS sprites.

    // Flip sprites after releasing the Rapier borrow.
    // `set_flip(horizontal, vertical)` — don't flip the ship upside down.
    for (_, sprite, player) in
        engine.iterate_two_components_mut::<SpriteRenderingComponent, PlayerComponent>()?
    {
        if let Some(horizontal_flip) = facing_updates[player.id as usize] {
            if sprite.flip_x != horizontal_flip {
                // About to mirror the sprite: mirror the player's x-facing flag as well.
                player.flip = -player.flip;
                sprite.set_flip(horizontal_flip, false);
            }
        }
    }

    // Feed this frame's player state into the particle emitters attached to the players, and
    // publish the emission-point height (entity position + the emitter's Y offset) for the UI.
    {
        let mut heights = emission_heights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (_, player, emitter, transform) in engine.iterate_three_components_mut::<
            PlayerComponent,
            ParticleSystemComponent,
            TransformComponent,
        >()? {
            let (thrusting, input_x, water_level) = emitter_states[player.id as usize];
            emitter.update_from_player(
                thrusting,
                input_x,
                player.flip,
                water_level,
                params.particle_surface_center,
                params.particle_surface_band,
                params.particle_underwater_depth,
            );
            heights[player.id as usize] =
                transform.position.y + emitter.spawn_offset_y - water_level;
        }
    }

    let mut bomb_positions: Vec<(f32, f32)> = Vec::new();
    for (_, transform_component, player_component, single_shot_audio) in
        engine.iterate_three_components_mut::<TransformComponent, PlayerComponent, SingleShotAudioComponent>()?
    {
        let player_index = player_component.id as usize;
        player_component.air_time = next_air_time[player_index];
        player_component.last_thrust_direction = next_thrust_direction[player_index];
        player_component.dive_hold_time = next_dive_hold_time[player_index];
        player_component.dive_was_held = next_dive_was_held[player_index];

        if controls[player_index].drop_bomb {
            //        add_bomb(engine, transform_component.position.x, transform_component.position.y)?;
            bomb_positions.push((
                transform_component.position.x,
                transform_component.position.y,
            ));
        }

        if controls[player_component.id as usize].horn_blare {
            schedule_blare(single_shot_audio)?;
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
