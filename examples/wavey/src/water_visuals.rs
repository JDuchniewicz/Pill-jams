use pill_engine::{define_global_component, game::*};

use crate::game::{GameState, WaterPoint};

// --- Water-surface driven procedural mesh ---

define_global_component!(PointsComponent {
    mesh_handle: MeshHandle,
});

const POINT_Z: f32 = 1.0;
/// Extra outline points added this far below each end of the water surface.
const WATER_BOTTOM_OFFSET: f32 = -100.0;
/// World-space span of the normalized water-point X coordinates (0..1).
const WATER_X_MIN: f32 = -100.0;
const WATER_X_MAX: f32 = 100.0;

/// Maps a normalized water-point X (0..1) to the world-space surface span.
fn water_world_x(normalized_x: f32) -> f32 {
    WATER_X_MIN + normalized_x * (WATER_X_MAX - WATER_X_MIN)
}

/// Creates the water body mesh, material and entity, registers the `PointsComponent` and adds
/// the per-frame update system.
pub fn start(engine: &mut Engine, active_scene: SceneHandle) -> Result<()> {
    let outline = {
        let game_state = engine.get_global_component::<GameState>()?;
        water_outline(&game_state.water_points)
    };
    let points_mesh_handle =
        engine.add_resource(Mesh::from_polygon("points_polygon", &outline)?)?;
    let points_material_handle = engine.add_resource::<Material>(
        Material::new("points_polygon_unlit")
            .albedo(Color::new(0.0, 0.0, 0.0))
            .emissive(Color::new(0.2, 0.6, 1.0)),
    )?;

    engine.add_global_component(PointsComponent {
        mesh_handle: points_mesh_handle,
    })?;

    engine.add_system("points_system", points_system)?;

    // Filled water body, rotated 180° about Y so its +Z-facing front faces the camera at −Z
    // (water_outline mirrors local X to keep rendered vertices aligned with the water points).
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

/// Builds the water body outline: the surface points plus a point `WATER_BOTTOM_OFFSET` below
/// each end. Local X is mirrored because the mesh entity is rotated 180° about Y to face the
/// camera, which keeps rendered vertices aligned with the stored water points.
fn water_outline(water_points: &[WaterPoint]) -> Vec<Vector2f> {
    if water_points.is_empty() {
        return Vec::new();
    }

    let first = &water_points[0];
    let last = &water_points[water_points.len() - 1];

    let mut outline = Vec::with_capacity(water_points.len() + 2);
    // Bottom-left corner.
    outline.push(Vector2f::new(
        -water_world_x(first.x),
        first.y + WATER_BOTTOM_OFFSET,
    ));
    // Water surface, left to right.
    for point in water_points {
        outline.push(Vector2f::new(-water_world_x(point.x), point.y));
    }
    // Bottom-right corner; the polygon closes back to the bottom-left.
    outline.push(Vector2f::new(
        -water_world_x(last.x),
        last.y + WATER_BOTTOM_OFFSET,
    ));

    outline
}

fn points_system(engine: &mut Engine) -> Result<()> {
    let (mesh_handle, outline, dots) = {
        let component = engine.get_global_component::<PointsComponent>()?;
        let game_state = engine.get_global_component::<GameState>()?;
        let dots = game_state
            .water_points
            .iter()
            .map(|point| Vector3f::new(water_world_x(point.x), point.y, POINT_Z))
            .collect::<Vec<_>>();
        (
            component.mesh_handle,
            water_outline(&game_state.water_points),
            dots,
        )
    };

    // Deform the filled water body to the current surface positions.
    engine.update_mesh(&mesh_handle, MeshData::from_polygon(&outline)?)?;

    // One primitive dot per simulated water point, always visible on top of the scene.
    for position in dots {
        engine.draw_point(
            position,
            PrimitiveStyle::overlay(Color::new(0.3, 1.0, 0.4)).with_point_size(1.0),
        )?;
    }

    Ok(())
}
