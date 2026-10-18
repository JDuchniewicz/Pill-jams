use pill_engine::{define_global_component, game::*};

use crate::duck::{spawn_floating_prop, FloatingPropKind};

/// World size of the container sprite quad (the container art is square, 47x47 px).
const CONTAINER_SPRITE_SIZE: f32 = 3.0;
/// Spawn position used by the "spawn a container" key.
const CONTAINER_SPAWN_POSITION: [f32; 3] = [50.0, 50.0, 0.0];

// Resources the spawn system needs, created once in `start`.
define_global_component!(ContainerSpawnerComponent {
    texture: TextureHandle,
});

pub fn start(engine: &mut Engine, _active_scene: SceneHandle) -> Result<()> {
    // Its own texture resource (container.png) drawn through the submerged sprite shader.
    let texture = engine.add_resource::<Texture>(Texture::new(
        "container",
        TextureType::Color,
        crate::assets::texture("container"),
    ))?;

    engine.add_global_component(ContainerSpawnerComponent { texture })?;
    engine.add_system("container_spawn_system", container_spawn_system)?;
    Ok(())
}

/// Press O to drop a container into the world at (50, 50).
fn container_spawn_system(engine: &mut Engine) -> Result<()> {
    let spawn_pressed = engine
        .get_global_component::<InputComponent>()?
        .get_key_pressed(KeyboardKey::KeyO);

    if spawn_pressed {
        spawn_container(engine)?;
    }

    Ok(())
}

/// Builds a container: same setup as the duck (and the player ships) minus inputs, particles and
/// audio.
pub fn spawn_container(engine: &mut Engine) -> Result<()> {
    let texture = engine
        .get_global_component::<ContainerSpawnerComponent>()?
        .texture;
    spawn_floating_prop(
        engine,
        texture,
        Vector3f::new(
            CONTAINER_SPAWN_POSITION[0],
            CONTAINER_SPAWN_POSITION[1],
            CONTAINER_SPAWN_POSITION[2],
        ),
        CONTAINER_SPRITE_SIZE,
        FloatingPropKind::Container,
    )
}
