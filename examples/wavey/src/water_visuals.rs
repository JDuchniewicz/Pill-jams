use pill_engine::{define_global_component, game::*};

use crate::water::{GameState, WaterPoint, WATER_X_MAX, WATER_X_MIN};
use crate::water_mesh::{WaterMeshComponent, WaterMeshPass};

// --- Water-surface driven procedural mesh ---

define_global_component!(PointsComponent {
    mesh_handle: MeshHandle,
});

const POINT_Z: f32 = 1.0;
/// World Y of the flat bottom edge of the water body.
const WATER_BOTTOM_Y: f32 = -100.0;

/// Maps a normalized water-point X (0..1) to the world-space surface span.
fn water_world_x(normalized_x: f32) -> f32 {
    WATER_X_MIN + normalized_x * (WATER_X_MAX - WATER_X_MIN)
}

/// Creates the water body mesh and entity, registers the `PointsComponent`, the water mesh pass
/// (`water_mesh_shader`) and the per-frame update system.
pub fn start(engine: &mut Engine, active_scene: SceneHandle) -> Result<()> {
    let (top, bottom, uv2) = {
        let game_state = engine.get_global_component::<GameState>()?;
        water_rows(&game_state.water_points)
    };
    let points_mesh_handle = engine.add_resource(Mesh::from_quad_strip_with_top_uv2(
        "points_polygon",
        &top,
        &bottom,
        &uv2,
    )?)?;

    // Custom-shader material: the water body is drawn by WaterMeshPass with `water_mesh_shader`.
    engine.register_component::<WaterMeshComponent>(active_scene)?;
    engine.add_render_pass(Box::new(WaterMeshPass::new()))?;

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
        .with_component(WaterMeshComponent {
            mesh: points_mesh_handle,
        })
        .build();

    Ok(())
}

/// Builds the water body as a quad strip: every surface point has a matching bottom point at the
/// same X, pinned to the flat bottom edge (`WATER_BOTTOM_Y`), and the two are connected by an
/// edge, so the mesh is made of quads.
///
/// Local X is mirrored because the mesh entity is rotated 180° about Y to face the camera, which
/// keeps rendered vertices aligned with the stored water points. The points are emitted right to
/// left so that, after the mirroring, local X increases along the strip (CCW winding).
///
/// Also builds the second UV set: `[horizontal velocity, 0]` per surface point.
fn water_rows(water_points: &[WaterPoint]) -> (Vec<Vector2f>, Vec<Vector2f>, Vec<[f32; 2]>) {
    let mut top = Vec::with_capacity(water_points.len());
    let mut bottom = Vec::with_capacity(water_points.len());
    let mut uv2 = Vec::with_capacity(water_points.len());

    for point in water_points.iter().rev() {
        let x = -water_world_x(point.x);
        top.push(Vector2f::new(x, point.y));
        // Flat bottom: every bottom vertex sits on the same horizontal line
        bottom.push(Vector2f::new(x, WATER_BOTTOM_Y));
        uv2.push([point.horizontal_velocity_delta, 0.0]);
    }

    (top, bottom, uv2)
}

fn points_system(engine: &mut Engine) -> Result<()> {
    let (mesh_handle, top, bottom, uv2, dots) = {
        let component = engine.get_global_component::<PointsComponent>()?;
        let game_state = engine.get_global_component::<GameState>()?;
        let dots = game_state
            .water_points
            .iter()
            .map(|point| Vector3f::new(water_world_x(point.x), point.y, POINT_Z))
            .collect::<Vec<_>>();
        let (top, bottom, uv2) = water_rows(&game_state.water_points);
        (component.mesh_handle, top, bottom, uv2, dots)
    };

    // Deform the filled water body to the current surface positions.
    // A malformed frame must not take the whole game down: skip the mesh update instead.
    match MeshData::from_quad_strip_with_top_uv2(&top, &bottom, &uv2) {
        Ok(mesh_data) => engine.update_mesh(&mesh_handle, mesh_data)?,
        Err(error) => {
            eprintln!("points_system: skipping invalid water mesh: {error}");
            return Ok(());
        }
    }

    // One primitive dot per simulated water point, always visible on top of the scene.
    for position in dots {
        engine.draw_point(
            position,
            PrimitiveStyle::overlay(Color::new(0.3, 1.0, 0.4)).with_point_size(1.0),
        )?;
    }

    Ok(())
}
