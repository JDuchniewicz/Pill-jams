use pill_engine::{define_global_component, game::*};

use crate::cartoon_pass::{CartoonComponent, CartoonPass};
use crate::game::{GameUIStateComponent, TagAlphaComponent};

// --- Point-cloud driven procedural mesh ---

const INCREASE_ORTHO_SIZE_KEY: KeyboardKey = KeyboardKey::KeyU;
const DECREASE_ORTHO_SIZE_KEY: KeyboardKey = KeyboardKey::KeyI;
const ORTHO_SIZE_SPEED: f32 = 100.0; // world units per second
const ORTHO_SIZE_MIN: f32 = 0.1;
const ORTHO_SIZE_MAX: f32 = 1000.0;

pub struct Point {
    /// Current position.
    pub position: Vector3f,
    /// Position the point was spawned at; movement is bounded around it.
    pub initial_position: Vector3f,
    /// Unit direction the point is currently wandering in (XY plane).
    pub direction: Vector3f,
}

define_global_component!(PointsComponent {
    points: Vec<Point>,
    mesh_handle: MeshHandle,
});

const POINT_COUNT: usize = 10;
const POINT_MIN_RADIUS: f32 = 0.6;
const POINT_MAX_RADIUS: f32 = 1.6;
const POINT_Z: f32 = 1.0;
const POINT_MOVE_SPEED: f32 = 1.0;
const POINT_MAX_DISTANCE: f32 = 10.0;

/// Sets up all rendering-related scene state: render settings, graphics components, custom
/// passes, resources, entities and the procedural polygon demo.
pub fn start(engine: &mut Engine, active_scene: SceneHandle) -> Result<()> {
    {
        let render_state = engine.get_global_component_mut::<RenderStateComponent>()?;
        render_state.background_color = [0.5, 0.5, 0.5];
        // Unlit and cartoon content is authored in display range; no HDR compression
        render_state.tonemap = Tonemap::None;
    }

    // Register graphics components
    engine.register_component::<TransformComponent>(active_scene)?;
    engine.register_component::<MeshRenderingComponent>(active_scene)?;
    engine.register_component::<CartoonComponent>(active_scene)?;
    engine.register_component::<CameraComponent>(active_scene)?;

    // Add render passes
    engine.add_render_pass(Box::new(CartoonPass::new()))?;

    // Add the main visuals system
    engine.add_system("visuals_system", visuals_system)?;
    engine.add_system("camera_zoom_system", camera_zoom_system)?;

    // --- Create resources ---

    let chimpanzini_bananini_mesh_handle = engine.add_resource(
        Mesh::new(
            "chimpanzini_bananini",
            "models/chimpanzini_bananini.obj".into(),
        )
        .with_uv_flip(true),
    )?;

    let chimpanzini_bananini_color_texture_handle =
        engine.add_resource::<Texture>(Texture::new(
            "chimpanzini_bananini",
            TextureType::Color,
            ResourceLoader::Path("textures/chimpanzini_bananini_color.png".into()),
        ))?;

    // Unlit look: black albedo (no lighting response) + texture as emission
    let chimpanzini_bananini_material_handle_unlit = engine.add_resource::<Material>(
        Material::new("chimpanzini_bananini_unlit")
            .albedo(Color::new(0.0, 0.0, 0.0))
            .emissive(Color::new(1.0, 1.0, 1.0))
            .emissive_texture(chimpanzini_bananini_color_texture_handle),
    )?;

    let chimpanzini_bananini_material_handle_lit = engine.add_resource::<Material>(
        Material::new("chimpanzini_bananini_lit")
            .albedo_texture(chimpanzini_bananini_color_texture_handle)
            .roughness(0.5),
    )?;

    // --- Create entities ---

    // Camera: looks down −Z, turned around to face the scene at the origin
    engine
        .build_entity(active_scene)
        .with_component(
            TransformComponent::builder()
                .position(Vector3f::new(0.0, 0.0, -3.0))
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

    engine
        .build_entity(active_scene)
        .with_component(
            TransformComponent::builder()
                .position(Vector3f::new(-1.0, 0.0, 0.0))
                .build(),
        )
        .with_component(
            MeshRenderingComponent::builder()
                .material(&chimpanzini_bananini_material_handle_lit)
                .mesh(&chimpanzini_bananini_mesh_handle)
                .build(),
        )
        .with_component(TagAlphaComponent {})
        .build();

    engine
        .build_entity(active_scene)
        .with_component(
            TransformComponent::builder()
                .position(Vector3f::new(1.0, 0.0, 0.0))
                .build(),
        )
        .with_component(
            MeshRenderingComponent::builder()
                .material(&chimpanzini_bananini_material_handle_unlit)
                .mesh(&chimpanzini_bananini_mesh_handle)
                .build(),
        )
        .with_component(TagAlphaComponent {})
        .build();

    // Drawn by the game-defined CartoonPass (posterize)
    engine
        .build_entity(active_scene)
        .with_component(
            TransformComponent::builder()
                .position(Vector3f::new(0.0, 0.0, 1.5))
                .build(),
        )
        .with_component(CartoonComponent {
            mesh: chimpanzini_bananini_mesh_handle,
            texture: chimpanzini_bananini_color_texture_handle,
            posterize_level: 3.0,
        })
        .with_component(TagAlphaComponent {})
        .build();

    // Procedural polygon driven by the PointsComponent
    setup_points_polygon(engine, active_scene)?;

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

    if let Some((center, right, up)) = camera_frame {
        // Rectangle corners: (-100, -100), (-100, 100), (100, 100), (100, -100)
        let corner = |x: f32, y: f32| center + right * x + up * y;
        let corners = [
            corner(-RECTANGLE_HALF_EXTENT, -RECTANGLE_HALF_EXTENT),
            corner(-RECTANGLE_HALF_EXTENT, RECTANGLE_HALF_EXTENT),
            corner(RECTANGLE_HALF_EXTENT, RECTANGLE_HALF_EXTENT),
            corner(RECTANGLE_HALF_EXTENT, -RECTANGLE_HALF_EXTENT),
        ];

        let style = PrimitiveStyle::overlay(Color::new(1.0, 0.7, 0.1));
        for i in 0..4 {
            engine.draw_line(corners[i], corners[(i + 1) % 4], style)?;
        }

        // Static red points at the rectangle corners.
        let point_style = PrimitiveStyle::overlay(Color::new(1.0, 0.0, 0.0));
        for corner in corners {
            engine.draw_point(corner, point_style)?;
        }
    }

    Ok(())
}

/// Creates the procedural polygon mesh, material and entity, registers the `PointsComponent`
/// and adds the per-frame update system.
fn setup_points_polygon(engine: &mut Engine, active_scene: SceneHandle) -> Result<()> {
    let points = random_points();
    let points_mesh_handle = engine.add_resource(Mesh::from_polygon(
        "points_polygon",
        &points_outline(&points),
    )?)?;
    let points_material_handle = engine.add_resource::<Material>(
        Material::new("points_polygon_unlit")
            .albedo(Color::new(0.0, 0.0, 0.0))
            .emissive(Color::new(0.2, 0.6, 1.0)),
    )?;

    engine.add_global_component(PointsComponent {
        points,
        mesh_handle: points_mesh_handle,
    })?;

    engine.add_system("points_system", points_system)?;

    // Filled polygon that follows the moving points. Rotated 180° about Y so its +Z-facing
    // front faces the camera at −Z (points_outline mirrors local X to keep rendered vertices
    // aligned with the stored point positions).
    engine
        .build_entity(active_scene)
        .with_component(
            TransformComponent::builder()
                .position(Vector3f::new(0.0, 0.0, POINT_Z))
                .rotation(Vector3f::new(0.0, 180.0, 0.0))
                .build(),
        )
        .with_component(
            MeshRenderingComponent::builder()
                .material(&points_material_handle)
                .mesh(&points_mesh_handle)
                .build(),
        )
        .build();

    Ok(())
}

/// Deterministic 32-bit hash mapped to [0, 1).
fn hash_unit(seed: u32) -> f32 {
    let mut x = seed.wrapping_add(0x9e37_79b9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x as f32 / u32::MAX as f32
}

fn random_points() -> Vec<Point> {
    (0..POINT_COUNT)
        .map(|i| {
            let angle = i as f32 / POINT_COUNT as f32 * std::f32::consts::TAU;
            let radius =
                POINT_MIN_RADIUS + (POINT_MAX_RADIUS - POINT_MIN_RADIUS) * hash_unit(i as u32 * 2);
            let position = Vector3f::new(angle.cos() * radius, angle.sin() * radius, POINT_Z);
            let direction_angle = hash_unit(i as u32 * 2 + 1) * std::f32::consts::TAU;
            let direction = Vector3f::new(direction_angle.cos(), direction_angle.sin(), 0.0);
            Point {
                position,
                initial_position: position,
                direction,
            }
        })
        .collect()
}

/// Points in angular order around their centroid; this keeps the outline a simple polygon even
/// after points start crossing each other. Local X is mirrored because the mesh entity is rotated
/// 180° about Y to face the camera.
fn points_outline(points: &[Point]) -> Vec<Vector2f> {
    if points.is_empty() {
        return Vec::new();
    }

    let centroid =
        points.iter().map(|point| point.position).sum::<Vector3f>() / points.len() as f32;
    let mut ordered: Vec<&Point> = points.iter().collect();
    ordered.sort_by(|a, b| {
        let angle_a = (a.position.y - centroid.y).atan2(a.position.x - centroid.x);
        let angle_b = (b.position.y - centroid.y).atan2(b.position.x - centroid.x);
        angle_a.total_cmp(&angle_b)
    });

    ordered
        .iter()
        .map(|point| Vector2f::new(-point.position.x, point.position.y))
        .collect()
}

fn points_system(engine: &mut Engine) -> Result<()> {
    let delta_time = engine.get_global_component::<TimeComponent>()?.delta_time;

    // Wander, bounded to POINT_MAX_DISTANCE around each point's spawn position.
    {
        let component = engine.get_global_component_mut::<PointsComponent>()?;
        for point in &mut component.points {
            point.position += point.direction * (POINT_MOVE_SPEED * delta_time);

            let offset = point.position - point.initial_position;
            if offset.length_squared() > POINT_MAX_DISTANCE * POINT_MAX_DISTANCE {
                let normal = offset.normalize();
                // Reflect the heading so the point keeps moving inside the allowed radius.
                point.direction =
                    (point.direction - 2.0 * point.direction.dot(normal) * normal).normalize();
                point.position = point.initial_position + normal * POINT_MAX_DISTANCE;
            }
        }
    }

    let (mesh_handle, outline, dots) = {
        let component = engine.get_global_component::<PointsComponent>()?;
        (
            component.mesh_handle,
            points_outline(&component.points),
            component
                .points
                .iter()
                .map(|point| point.position)
                .collect::<Vec<_>>(),
        )
    };

    // Deform the filled polygon to the current point positions.
    engine.update_mesh(&mesh_handle, MeshData::from_polygon(&outline)?)?;

    // One primitive dot per point, always visible on top of the scene.
    for position in dots {
        engine.draw_point(position, PrimitiveStyle::overlay(Color::new(0.1, 1.0, 0.4)))?;
    }

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
