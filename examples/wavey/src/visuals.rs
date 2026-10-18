use pill_engine::game::*;

use crate::cartoon_pass::CartoonPass;
use crate::game::GameUIStateComponent;

// --- Camera / reference visuals ---

const INCREASE_ORTHO_SIZE_KEY: KeyboardKey = KeyboardKey::KeyU;
const DECREASE_ORTHO_SIZE_KEY: KeyboardKey = KeyboardKey::KeyI;
const ORTHO_SIZE_SPEED: f32 = 100.0; // world units per second
const ORTHO_SIZE_MIN: f32 = 0.1;
const ORTHO_SIZE_MAX: f32 = 1000.0;

/// Sets up all rendering-related scene state: render settings, graphics components, custom
/// passes, resources, entities and the procedural polygon demo.
pub fn start(engine: &mut Engine, active_scene: SceneHandle) -> Result<()> {
    {
        let render_state = engine.get_global_component_mut::<RenderStateComponent>()?;
        render_state.background_color = [0.5, 0.5, 0.5];
        // Unlit and cartoon content is authored in display range; no HDR compression
        render_state.tonemap = Tonemap::None;
    }

    // Add render passes
    engine.add_render_pass(Box::new(CartoonPass::new()))?;

    // Add the main visuals system
    engine.add_system("visuals_system", visuals_system)?;
    engine.add_system("camera_zoom_system", camera_zoom_system)?;
    engine.add_system("sprite_bounds_system", sprite_bounds_system)?;

    // --- Create resources ---

    // --- Create entities ---

    // Camera: looks down −Z, turned around to face the scene at the origin
    engine
        .build_entity(active_scene)
        .with_component(
            TransformComponent::builder()
                .position(Vector3f::new(0.0, 30.0, -30.0))
                .rotation(Vector3f::new(0.0, 180.0, 0.0))
                .build(),
        )
        .with_component(
            CameraComponent::builder()
                .enabled(true)
                // Large enough to fit the ±100 reference rectangle at depth 10.
                .orthographic(200.0)
                .build(),
        )
        .build();

    // Water body driven by GameState::water_points
    crate::water_visuals::start(engine, active_scene)?;

    Ok(())
}

/// Main visuals system: draws world-space reference geometry every frame.
fn visuals_system(engine: &mut Engine) -> Result<()> {
    // --- AABB rectangle in the camera plane, 10 units from the camera ---
    const RECTANGLE_DEPTH: f32 = 10.0;
    const RECTANGLE_HALF_EXTENT: f32 = 100.0;

    let mut camera_frame = None;
    for (_, transform, camera) in
        engine.iterate_two_components::<TransformComponent, CameraComponent>()?
    {
        if camera.enabled {
            camera_frame = Some((
                transform.position + transform.get_forward_direction() * RECTANGLE_DEPTH,
                transform.get_right_direction(),
                transform.get_up_direction(),
            ));
            break;
        }
    }

    //if let Some((center, right, up)) = camera_frame {
    //    // Rectangle corners: (-100, -100), (-100, 100), (100, 100), (100, -100)
    //    let corner = |x: f32, y: f32| center + right * x + up * y;
    //    let corners = [
    //        corner(-RECTANGLE_HALF_EXTENT, -RECTANGLE_HALF_EXTENT),
    //        corner(-RECTANGLE_HALF_EXTENT, RECTANGLE_HALF_EXTENT),
    //        corner(RECTANGLE_HALF_EXTENT, RECTANGLE_HALF_EXTENT),
    //        corner(RECTANGLE_HALF_EXTENT, -RECTANGLE_HALF_EXTENT),
    //    ];
    //
    //    let style = PrimitiveStyle::overlay(Color::new(1.0, 0.7, 0.1));
    //    for i in 0..4 {
    //        engine.draw_line(corners[i], corners[(i + 1) % 4], style)?;
    //    }
    //
    //    // Static red points at the rectangle corners.
    //    let point_style = PrimitiveStyle::overlay(Color::new(1.0, 0.0, 0.0));
    //    for corner in corners {
    //        engine.draw_point(corner, point_style)?;
    //    }
    //}

    Ok(())
}

/// Increases/decreases the active camera's orthographic vertical size with U/I.
fn camera_zoom_system(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;

    let change = {
        let input_component = engine.get_global_component::<InputComponent>()?;
        let mut change = 0.0;
        if input_component.get_key(INCREASE_ORTHO_SIZE_KEY) {
            change += 1.0;
        }
        if input_component.get_key(DECREASE_ORTHO_SIZE_KEY) {
            change -= 1.0;
        }
        change
    };

    let mut ortho_size = None;
    for (_, camera_component) in engine.iterate_one_component_mut::<CameraComponent>()? {
        if !camera_component.enabled {
            continue;
        }
        if let CameraProjection::Orthographic { vertical_size } = &mut camera_component.projection {
            if change != 0.0 {
                *vertical_size = (*vertical_size + change * ORTHO_SIZE_SPEED * delta_time)
                    .clamp(ORTHO_SIZE_MIN, ORTHO_SIZE_MAX);
            }
            ortho_size = Some(*vertical_size);
        }
    }

    // TODO: implement later when implementing bigger screen sizes
    // TODO: for now two players
    // if the distance between two players is wider than screen size (- 2x some margin)
    // we need to zoom out smoothly -> this also needs to be projected to screen size

    if let Some(ortho_size) = ortho_size {
        if let Ok(state) = engine.get_global_component::<GameUIStateComponent>() {
            *state
                .ortho_size
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = ortho_size;
        }
    }

    Ok(())
}

/// Debug overlay driven by the "Sprite bounds" checkbox in the Wavey window: draws a primitive
/// rectangle around every sprite, matching the quad the sprite pass builds.
fn sprite_bounds_system(engine: &mut Engine) -> Result<()> {
    let enabled = {
        let Ok(state) = engine.get_global_component::<GameUIStateComponent>() else {
            return Ok(());
        };
        *state
            .draw_sprite_bounds
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    };
    if !enabled {
        return Ok(());
    }

    // Same camera basis PassSprites uses for the quad, so the outline matches exactly.
    let mut camera_frame = None;
    for (_, transform, camera) in
        engine.iterate_two_components::<TransformComponent, CameraComponent>()?
    {
        if camera.enabled {
            camera_frame = Some((
                transform.get_right_direction(),
                transform.get_up_direction(),
            ));
            break;
        }
    }
    let Some((right, up)) = camera_frame else {
        return Ok(());
    };

    // Collect first: the component iteration borrows the engine, drawing borrows it again.
    let mut outlines = Vec::new();
    for (_, transform, sprite) in
        engine.iterate_two_components::<TransformComponent, SpriteRenderingComponent>()?
    {
        if sprite.texture_handle.is_none() {
            continue;
        }
        let half_size = Vector2f::new(
            sprite.size.x * transform.scale.x * 0.5,
            sprite.size.y * transform.scale.y * 0.5,
        );
        if half_size.x <= 0.0 || half_size.y <= 0.0 {
            continue;
        }
        let center = transform.position;
        let corner = |x: f32, y: f32| center + right * x + up * y;
        outlines.push([
            corner(-half_size.x, -half_size.y),
            corner(-half_size.x, half_size.y),
            corner(half_size.x, half_size.y),
            corner(half_size.x, -half_size.y),
        ]);
    }

    let style = PrimitiveStyle::overlay(Color::new(1.0, 0.0, 1.0));
    for corners in outlines {
        for i in 0..4 {
            engine.draw_line(corners[i], corners[(i + 1) % 4], style)?;
        }
    }

    Ok(())
}
