use pill_engine::{define_global_component, game::*};

use crate::player::{
    apply_float_ballast, ship_buoyancy_points, ship_hull_shape, PlayerPhysicsComponent,
    HULL_GEOMETRY_MASS_FRACTION,
};
use crate::water_floating::WaterFloatingComponent;

/// World size of the duck sprite quad (the duck art is almost square).
const DUCK_SPRITE_SIZE: f32 = 20.0;
/// Spawn position used by the "spawn a duck" key.
const DUCK_SPAWN_POSITION: [f32; 3] = [50.0, 50.0, 0.0];

// Resources the spawn system needs, created once in `start`.
define_global_component!(DuckSpawnerComponent {
    texture: TextureHandle,
});

pub fn start(engine: &mut Engine, _active_scene: SceneHandle) -> Result<()> {
    // Its own texture resource (duck.png) drawn through the submerged sprite shader.
    let texture = engine.add_resource::<Texture>(Texture::new(
        "duck",
        TextureType::Color,
        ResourceLoader::Path("textures/duck.png".into()),
    ))?;

    engine.add_global_component(DuckSpawnerComponent { texture })?;
    engine.add_system("duck_spawn_system", duck_spawn_system)?;
    Ok(())
}

/// Press P to drop a duck into the world at (50, 50).
fn duck_spawn_system(engine: &mut Engine) -> Result<()> {
    let spawn_pressed = engine
        .get_global_component::<InputComponent>()?
        .get_key_pressed(KeyboardKey::KeyP);

    if spawn_pressed {
        spawn_duck(engine)?;
    }

    Ok(())
}

/// Builds a duck: sprite + rigid body + hull collider + water floating, i.e. the same setup as a
/// player ship, minus inputs, particles and audio.
pub fn spawn_duck(engine: &mut Engine) -> Result<()> {
    let texture = engine
        .get_global_component::<DuckSpawnerComponent>()?
        .texture;
    let params = {
        let shared = engine.get_global_component::<PlayerPhysicsComponent>()?;
        let guard = shared
            .params
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    };
    let active_scene = engine.get_active_scene_handle()?;

    let duck = engine
        .build_entity(active_scene)
        .with_component(
            TransformComponent::builder()
                .position(Vector3f::new(
                    DUCK_SPAWN_POSITION[0],
                    DUCK_SPAWN_POSITION[1],
                    DUCK_SPAWN_POSITION[2],
                ))
                .build(),
        )
        .with_component(
            SpriteRenderingComponent::builder()
                .texture(&texture)
                .size(Vector2f::new(DUCK_SPRITE_SIZE, DUCK_SPRITE_SIZE))
                // 2D game objects stay visible over the water
                .depth_test(false)
                // Hull below the water surface is drawn at half brightness
                .submerge(0.5, 0.0)
                .build(),
        )
        .with_component(
            RigidBodyComponent::builder()
                .body_type(RigidBodyType::Dynamic)
                .linear_damping(params.linear_damping)
                .angular_damping(params.angular_damping)
                .locked_axes(
                    LockedAxes::TRANSLATION_LOCKED_Z
                        | LockedAxes::ROTATION_LOCKED_X
                        | LockedAxes::ROTATION_LOCKED_Y,
                )
                .ccd_enabled(params.ccd_enabled)
                .build(),
        )
        .with_component(
            ColliderComponent::builder()
                .shape(ship_hull_shape(&params))
                .friction(params.friction)
                .restitution(params.restitution)
                .mass(params.ship_mass * HULL_GEOMETRY_MASS_FRACTION)
                .build(),
        )
        .with_component(WaterFloatingComponent::new(ship_buoyancy_points(&params)))
        .build();

    // Same low ballast as the ships, so the duck floats upright instead of capsizing.
    apply_float_ballast(engine, duck, &params)?;

    Ok(())
}
