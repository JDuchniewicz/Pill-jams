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

/// Shared builder for the non-player floating props (ducks, containers): sprite + rigid body +
/// hull collider + water floating, i.e. the same setup as a player ship, minus inputs, particles
/// and audio. The sprite uses the submerged sprite shader.
#[derive(Clone, Copy, Debug)]
pub enum FloatingPropKind {
    Duck,
    Container,
}

/// Builds a duck at the duck spawn position.
pub fn spawn_duck(engine: &mut Engine) -> Result<()> {
    let texture = engine
        .get_global_component::<DuckSpawnerComponent>()?
        .texture;
    spawn_floating_prop(
        engine,
        texture,
        Vector3f::new(
            DUCK_SPAWN_POSITION[0],
            DUCK_SPAWN_POSITION[1],
            DUCK_SPAWN_POSITION[2],
        ),
        DUCK_SPRITE_SIZE,
        FloatingPropKind::Duck,
    )
}

/// Builds a non-player floating prop with physics matching its sprite.
/// Ships, ducks, and containers no longer share collision geometry.
pub fn spawn_floating_prop(
    engine: &mut Engine,
    texture: TextureHandle,
    position: Vector3f,
    sprite_size: f32,
    kind: FloatingPropKind,
) -> Result<()> {
    let params = {
        let shared = engine.get_global_component::<PlayerPhysicsComponent>()?;
        let guard = shared
            .params
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    };

    let active_scene = engine.get_active_scene_handle()?;

    // Collider half-extents and mass in world units.
    let (shape, half_width, half_height, mass) = match kind {
        FloatingPropKind::Container => {
            // Square crate, slightly inset from the 3x3 sprite.
            // At sprite_size=3: collider is 2.7 x 2.7 world units.
            let half_width = sprite_size * 0.30;
            let half_height = sprite_size * 0.30;
            let half_depth = sprite_size * 0.10;

            (
                SharedShape::cuboid(half_width, half_height, half_depth),
                half_width,
                half_height,
                3.0,
            )
        }

        FloatingPropKind::Duck => {
            // Horizontal capsule approximating a duck's body.
            // Smaller and rounder than the container collider.
            let radius = sprite_size * 0.22;
            let half_segment = sprite_size * 0.14;

            (
                SharedShape::capsule_x(half_segment, radius),
                half_segment + radius,
                radius,
                1.2,
            )
        }
    };

    // Three buoyancy samples relative to the PROP's actual dimensions.
    // Previously these used the 20-unit ship geometry!
    let buoyancy_points = [
        Vector3f::new(-half_width * 0.65, -half_height * 0.80, 0.0),
        Vector3f::new(0.0, -half_height * 0.80, 0.0),
        Vector3f::new(half_width * 0.65, -half_height * 0.80, 0.0),
    ];

    engine
        .build_entity(active_scene)
        .with_component(TransformComponent::builder().position(position).build())
        .with_component(
            SpriteRenderingComponent::builder()
                .texture(&texture)
                .size(Vector2f::new(sprite_size, sprite_size))
                .depth_test(false)
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
                .shape(shape)
                .friction(params.friction)
                .restitution(params.restitution)
                .mass(mass)
                .build(),
        )
        .with_component(WaterFloatingComponent::new(buoyancy_points))
        .build();

    Ok(())
}
