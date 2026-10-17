use crate::{
    ecs::MeshRenderingComponent,
    engine::Engine,
    renderer::resources::RendererMesh,
    resources::{Resource, ResourceStorage},
};

use pill_core::{
    get_type_name, EngineError, PillSlotMapKey, PillStyle, PillTypeMapKey, Vector2f, Vector3f,
};

use pill_core::{ErrorContext, Result};
use std::path::{Path, PathBuf};

#[cfg(feature = "obj_loading")]
fn obj_load_options() -> tobj::LoadOptions {
    tobj::LoadOptions {
        triangulate: true,
        single_index: true,
        ..Default::default()
    }
}

pill_core::define_new_pill_slotmap_key! {
    pub struct MeshHandle;
}

#[readonly::make]
pub struct Mesh {
    #[readonly]
    pub name: String,
    #[readonly]
    pub path: PathBuf,
    mesh_data: Option<MeshData>,

    // When exporting from Blender, V coordinate is flipped, so we need to flip it back
    // Should be set to false when importing a mesh exported as obj from Blender
    flip_uv_y: bool,
}

// TODO: Add posibility to load from bytes using ResourceLoader
impl Mesh {
    pub fn new(name: &str, path: PathBuf) -> Self {
        Self {
            name: name.to_string(),
            path,
            mesh_data: None,
            flip_uv_y: false,
        }
    }

    pub fn with_uv_flip(mut self, flip: bool) -> Self {
        self.flip_uv_y = flip;
        self
    }

    pub fn from_data(name: &str, mesh_data: MeshData) -> Self {
        Self {
            name: name.to_string(),
            path: PathBuf::new(),
            mesh_data: Some(mesh_data),
            flip_uv_y: false,
        }
    }

    pub fn cube(name: &str, size: f32) -> Self {
        Self::from_data(name, MeshData::cube(size))
    }

    /// Replaces the CPU-side geometry used by `Engine::update_mesh`.
    pub(crate) fn set_mesh_data(&mut self, mesh_data: MeshData) {
        self.mesh_data = Some(mesh_data);
    }

    /// Creates a flat, filled mesh from a closed polyline in the XY plane.
    ///
    /// The polyline is treated as a loop: the last point connects back to the first, and a
    /// duplicated closing point is allowed but not required. Convex and concave simple polygons
    /// are supported (ear-clipping triangulation); the outline must not intersect itself and
    /// must enclose a non-zero area. Holes are not supported.
    ///
    /// The generated geometry lies at `z = 0` with +Z normals. Winding is normalized to
    /// counter-clockwise, so the front face points toward +Z (the engine culls back faces).
    /// Texture coordinates span the polygon's bounding box.
    pub fn from_polygon(name: &str, points: &[Vector2f]) -> Result<Self> {
        Ok(Self::from_data(name, MeshData::from_polygon(points)?))
    }

    /// [`Mesh::from_polygon`] plus a second UV set (`TEXCOORD1`), one value per input point.
    /// The values follow the outline through the same normalization as the positions.
    pub fn from_polygon_with_uv2(
        name: &str,
        points: &[Vector2f],
        uv2: &[[f32; 2]],
    ) -> Result<Self> {
        Ok(Self::from_data(
            name,
            MeshData::from_polygon_with_uv2(points, uv2)?,
        ))
    }

    /// Quad strip between two rows of matching points (see [`MeshData::from_quad_strip`]).
    pub fn from_quad_strip(name: &str, top: &[Vector2f], bottom: &[Vector2f]) -> Result<Self> {
        Ok(Self::from_data(
            name,
            MeshData::from_quad_strip(top, bottom)?,
        ))
    }

    /// Quad strip with a second UV set for the top row (bottom row vertices get `[0, 0]`).
    pub fn from_quad_strip_with_top_uv2(
        name: &str,
        top: &[Vector2f],
        bottom: &[Vector2f],
        uv2_top: &[[f32; 2]],
    ) -> Result<Self> {
        Ok(Self::from_data(
            name,
            MeshData::from_quad_strip_with_top_uv2(top, bottom, uv2_top)?,
        ))
    }

    /// Build a mesh from pre-converted `.cooked_mesh` bytes (e.g. `include_bytes!(...)`).
    /// Works on all targets including wasm. Produces smaller binaries than
    /// `from_obj_bytes` — run `pill_launcher -a assets` to generate `.cooked_mesh` files.
    pub fn from_cooked_mesh_bytes(name: &str, bytes: &[u8]) -> Result<Self> {
        Ok(Self::from_data(name, load_cooked_mesh(bytes)?))
    }

    /// Parse a mesh from raw OBJ bytes; use `include_bytes!` to bundle assets into the binary (required on WASM).
    #[cfg(feature = "obj_loading")]
    pub fn from_obj_bytes(name: &str, bytes: &[u8]) -> Result<Self> {
        Ok(Self::from_data(
            name,
            MeshData::from_obj_bytes(bytes, false)?,
        ))
    }
}

impl PillTypeMapKey for Mesh {
    type Storage = ResourceStorage<Mesh>;
}

fn load_cooked_mesh(bytes: &[u8]) -> Result<MeshData> {
    if bytes.len() < 16 || &bytes[0..4] != b"RMSH" {
        return Err("not a valid .cooked_mesh file (bad magic or truncated header)".into());
    }
    let vertex_count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let index_count = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let vertex_size = std::mem::size_of::<MeshVertex>();
    let expected = 16 + vertex_count * vertex_size + index_count * 4;
    if bytes.len() < expected {
        return Err(format!(
            ".cooked_mesh truncated: expected {} bytes, got {}",
            expected,
            bytes.len()
        )
        .into());
    }
    let vertex_bytes = &bytes[16..16 + vertex_count * vertex_size];
    let index_bytes = &bytes[16 + vertex_count * vertex_size..expected];
    // cast_slice requires the source pointer to be aligned to the target type.
    // include_bytes! data is only 1-byte aligned, so collect through Vec<u32>
    // (which allocates at 4-byte alignment) before casting to MeshVertex / u32.
    let vertex_u32s: Vec<u32> = vertex_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&bytes| u32::from_le_bytes(bytes))
        .collect();
    let vertices: Vec<MeshVertex> = bytemuck::cast_slice(&vertex_u32s).to_vec();
    let indices: Vec<u32> = index_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&bytes| u32::from_le_bytes(bytes))
        .collect();
    Ok(MeshData {
        vertices,
        indices,
        texture_coordinates_2: Vec::new(),
    })
}

impl Resource for Mesh {
    type Handle = MeshHandle;

    fn get_name(&self) -> String {
        self.name.clone()
    }

    fn initialize(&mut self, engine: &mut Engine) -> Result<()> {
        let error_message = format!(
            "Initializing {} {} failed",
            "Resource".general_object_style(),
            get_type_name::<Self>().specific_object_style()
        );

        // Load from file only if mesh_data not already set (procedural mesh)
        if self.mesh_data.is_none() {
            let base = engine.game_resources_directory_path.join(&self.path);
            let cooked_mesh_path = base.with_extension("cooked_mesh");

            if cooked_mesh_path.exists() {
                let bytes =
                    std::fs::read(&cooked_mesh_path).map_err(|e| -> pill_core::PillError {
                        format!("Failed to read mesh {cooked_mesh_path:?}: {e}").into()
                    })?;
                let mut mesh_data = load_cooked_mesh(&bytes).context(error_message.clone())?;
                // Cooked OBJ meshes store 1 - v; with_uv_flip asks for the raw v (same as the OBJ path)
                if self.flip_uv_y {
                    for vertex in &mut mesh_data.vertices {
                        vertex.texture_coordinates[1] = 1.0 - vertex.texture_coordinates[1];
                        vertex.bitangent = vertex.bitangent.map(|value| -value);
                    }
                }
                self.mesh_data = Some(mesh_data);
            } else {
                #[cfg(feature = "obj_loading")]
                {
                    pill_core::validate_asset_path(&base, &["obj"])
                        .context(error_message.clone())?;
                    let mesh_data = MeshData::new(&base, self.flip_uv_y)
                        .context(error_message.clone())
                        .context(format!(
                            "Failed to create mesh data from {} file",
                            base.file_name().unwrap().to_string_lossy()
                        ))?;
                    self.mesh_data = Some(mesh_data);
                }
                #[cfg(not(feature = "obj_loading"))]
                return Err(pill_core::PillError::from(format!(
                    "No preprocessed .cooked_mesh found for {:?}; run `pill_launcher -a assets`",
                    base
                )));
            }
        }

        #[cfg(not(feature = "headless"))]
        {
            let renderer_mesh = RendererMesh::new(
                engine.renderer.get_device(),
                &self.name,
                self.mesh_data.as_ref().unwrap(),
            )
            .context(error_message)?;
            engine.resource_manager.add_resource(renderer_mesh)?;
        }

        Ok(())
    }

    fn destroy<H: PillSlotMapKey>(&mut self, engine: &mut Engine, self_handle: H) -> Result<()> {
        // Destroy renderer counterpart
        #[cfg(not(feature = "headless"))]
        engine
            .resource_manager
            .remove_resource_by_name::<RendererMesh>(&self.name)?;

        // Null out any MeshRenderingComponent that references this mesh
        for (_scene_handle, scene) in engine.scene_manager.scenes.iter_mut() {
            if let Ok(iter) = scene.get_one_component_iterator_mut::<MeshRenderingComponent>() {
                for (_entity_handle, mesh_rendering_component) in iter {
                    if let Some(mesh_handle) = mesh_rendering_component.mesh_handle {
                        if mesh_handle.data() == self_handle.data() {
                            mesh_rendering_component.set_mesh_handle(Option::<MeshHandle>::None);
                            mesh_rendering_component
                                .update_render_queue_key(&engine.resource_manager)
                                .unwrap();
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[repr(C)]
// bytemuck::Pod indicates that Vertex is "Plain Old Data", and thus can be interpretted as a &[u8]
// bytemuck::Zeroable indicates that Vertex can be used with std::mem::zeroed()
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    position: [f32; 3],
    texture_coordinates: [f32; 2],
    normal: [f32; 3],
    tangent: [f32; 3],
    bitangent: [f32; 3],
}

pub struct MeshData {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    /// Optional second UV set (`TEXCOORD1`), one entry per vertex. Empty when unused.
    /// Kept as a separate stream so the primary vertex layout stays unchanged.
    pub texture_coordinates_2: Vec<[f32; 2]>,
}

impl MeshData {
    #[cfg(feature = "obj_loading")]
    pub fn new(path: &Path, flip_uv_y: bool) -> Result<Self> {
        let (models, _materials) = tobj::load_obj(path, &obj_load_options())?;
        Self::from_tobj_models(models, &path.display().to_string(), flip_uv_y)
    }

    #[cfg(feature = "obj_loading")]
    pub fn from_obj_bytes(bytes: &[u8], flip_uv_y: bool) -> Result<Self> {
        let mut reader = std::io::Cursor::new(bytes);
        let (models, _materials) = tobj::load_obj_buf(&mut reader, &obj_load_options(), |_| {
            Err(tobj::LoadError::OpenFileFailed)
        })?;
        Self::from_tobj_models(models, "<in-memory>", flip_uv_y)
    }

    #[cfg(feature = "obj_loading")]
    fn from_tobj_models(models: Vec<tobj::Model>, source: &str, flip_uv_y: bool) -> Result<Self> {
        // Check data validity
        if models.len() > 1 {
            return Err(EngineError::InvalidModelFileMultipleMeshes(source.to_string()).into());
        }

        if models.is_empty() {
            return Err(EngineError::InvalidModelFile(source.to_string()).into());
        }

        // Load vertex data from model
        let mesh = &models[0].mesh;

        // Read vertices
        let mut vertices = Vec::new();
        for i in 0..mesh.positions.len() / 3 {
            let uv_y = *mesh.texcoords.get(i * 2 + 1).unwrap_or(&0.0);
            let final_uv_y = if flip_uv_y { uv_y } else { 1.0 - uv_y };

            vertices.push(MeshVertex {
                position: [
                    mesh.positions[i * 3],
                    mesh.positions[i * 3 + 1],
                    mesh.positions[i * 3 + 2],
                ],
                texture_coordinates: [
                    // Blender uses V coordinate flipped
                    *mesh.texcoords.get(i * 2).unwrap_or(&0.0),
                    final_uv_y,
                ],
                normal: [
                    mesh.normals[i * 3],
                    mesh.normals[i * 3 + 1],
                    mesh.normals[i * 3 + 2],
                ],
                tangent: [0.0; 3],
                bitangent: [0.0; 3],
            });
        }

        // Read indices
        let indices = &mesh.indices;
        let mut triangles_included = (0..vertices.len()).collect::<Vec<_>>();

        // Calculate tangents and bitangets
        for c in indices.chunks(3) {
            let v0 = vertices[c[0] as usize];
            let v1 = vertices[c[1] as usize];
            let v2 = vertices[c[2] as usize];

            let pos0: Vector3f = v0.position.into();
            let pos1: Vector3f = v1.position.into();
            let pos2: Vector3f = v2.position.into();

            let uv0: Vector2f = v0.texture_coordinates.into();
            let uv1: Vector2f = v1.texture_coordinates.into();
            let uv2: Vector2f = v2.texture_coordinates.into();

            // Calculate the edges of the triangle
            let delta_pos1 = pos1 - pos0;
            let delta_pos2 = pos2 - pos0;

            // Calculate the direction needed to calculate the tangent and bitangent
            let delta_uv1 = uv1 - uv0;
            let delta_uv2 = uv2 - uv0;

            // Calculate tangent and bitangent
            let r = 1.0 / (delta_uv1.x * delta_uv2.y - delta_uv1.y * delta_uv2.x);
            let tangent = (delta_pos1 * delta_uv2.y - delta_pos2 * delta_uv1.y) * r;
            let bitangent = (delta_pos2 * delta_uv1.x - delta_pos1 * delta_uv2.x) * r;

            // Assign same tangent/bitangent to each vertex in the triangle
            vertices[c[0] as usize].tangent =
                (tangent + Vector3f::from(vertices[c[0] as usize].tangent)).into();
            vertices[c[1] as usize].tangent =
                (tangent + Vector3f::from(vertices[c[1] as usize].tangent)).into();
            vertices[c[2] as usize].tangent =
                (tangent + Vector3f::from(vertices[c[2] as usize].tangent)).into();
            vertices[c[0] as usize].bitangent =
                (bitangent + Vector3f::from(vertices[c[0] as usize].bitangent)).into();
            vertices[c[1] as usize].bitangent =
                (bitangent + Vector3f::from(vertices[c[1] as usize].bitangent)).into();
            vertices[c[2] as usize].bitangent =
                (bitangent + Vector3f::from(vertices[c[2] as usize].bitangent)).into();

            // Prepare data for averaging tangents and bitangents
            triangles_included[c[0] as usize] += 1;
            triangles_included[c[1] as usize] += 1;
            triangles_included[c[2] as usize] += 1;
        }

        // Average the tangents and bitangents
        for (i, n) in triangles_included.into_iter().enumerate() {
            let denom = 1.0 / n as f32;
            let vertex = &mut vertices[i];
            vertex.tangent = (Vector3f::from(vertex.tangent) * denom).normalize().into();
            vertex.bitangent = (Vector3f::from(vertex.bitangent) * denom)
                .normalize()
                .into();
        }

        let mesh_data = MeshData {
            vertices,
            indices: mesh.indices.clone(),
            texture_coordinates_2: Vec::new(),
        };

        Ok(mesh_data)
    }

    pub fn cube(size: f32) -> Self {
        let s = size / 2.0;

        // 6 faces, 4 vertices each = 24 vertices (each face has unique normals)
        let vertices = vec![
            // Front face (Z+)
            MeshVertex {
                position: [-s, -s, s],
                texture_coordinates: [0.0, 1.0],
                normal: [0.0, 0.0, 1.0],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            MeshVertex {
                position: [s, -s, s],
                texture_coordinates: [1.0, 1.0],
                normal: [0.0, 0.0, 1.0],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            MeshVertex {
                position: [s, s, s],
                texture_coordinates: [1.0, 0.0],
                normal: [0.0, 0.0, 1.0],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            MeshVertex {
                position: [-s, s, s],
                texture_coordinates: [0.0, 0.0],
                normal: [0.0, 0.0, 1.0],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            // Back face (Z-)
            MeshVertex {
                position: [s, -s, -s],
                texture_coordinates: [0.0, 1.0],
                normal: [0.0, 0.0, -1.0],
                tangent: [-1.0, 0.0, 0.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            MeshVertex {
                position: [-s, -s, -s],
                texture_coordinates: [1.0, 1.0],
                normal: [0.0, 0.0, -1.0],
                tangent: [-1.0, 0.0, 0.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            MeshVertex {
                position: [-s, s, -s],
                texture_coordinates: [1.0, 0.0],
                normal: [0.0, 0.0, -1.0],
                tangent: [-1.0, 0.0, 0.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            MeshVertex {
                position: [s, s, -s],
                texture_coordinates: [0.0, 0.0],
                normal: [0.0, 0.0, -1.0],
                tangent: [-1.0, 0.0, 0.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            // Top face (Y+)
            MeshVertex {
                position: [-s, s, s],
                texture_coordinates: [0.0, 1.0],
                normal: [0.0, 1.0, 0.0],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 0.0, -1.0],
            },
            MeshVertex {
                position: [s, s, s],
                texture_coordinates: [1.0, 1.0],
                normal: [0.0, 1.0, 0.0],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 0.0, -1.0],
            },
            MeshVertex {
                position: [s, s, -s],
                texture_coordinates: [1.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 0.0, -1.0],
            },
            MeshVertex {
                position: [-s, s, -s],
                texture_coordinates: [0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 0.0, -1.0],
            },
            // Bottom face (Y-)
            MeshVertex {
                position: [-s, -s, -s],
                texture_coordinates: [0.0, 1.0],
                normal: [0.0, -1.0, 0.0],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 0.0, 1.0],
            },
            MeshVertex {
                position: [s, -s, -s],
                texture_coordinates: [1.0, 1.0],
                normal: [0.0, -1.0, 0.0],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 0.0, 1.0],
            },
            MeshVertex {
                position: [s, -s, s],
                texture_coordinates: [1.0, 0.0],
                normal: [0.0, -1.0, 0.0],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 0.0, 1.0],
            },
            MeshVertex {
                position: [-s, -s, s],
                texture_coordinates: [0.0, 0.0],
                normal: [0.0, -1.0, 0.0],
                tangent: [1.0, 0.0, 0.0],
                bitangent: [0.0, 0.0, 1.0],
            },
            // Right face (X+)
            MeshVertex {
                position: [s, -s, s],
                texture_coordinates: [0.0, 1.0],
                normal: [1.0, 0.0, 0.0],
                tangent: [0.0, 0.0, -1.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            MeshVertex {
                position: [s, -s, -s],
                texture_coordinates: [1.0, 1.0],
                normal: [1.0, 0.0, 0.0],
                tangent: [0.0, 0.0, -1.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            MeshVertex {
                position: [s, s, -s],
                texture_coordinates: [1.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                tangent: [0.0, 0.0, -1.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            MeshVertex {
                position: [s, s, s],
                texture_coordinates: [0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                tangent: [0.0, 0.0, -1.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            // Left face (X-)
            MeshVertex {
                position: [-s, -s, -s],
                texture_coordinates: [0.0, 1.0],
                normal: [-1.0, 0.0, 0.0],
                tangent: [0.0, 0.0, 1.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            MeshVertex {
                position: [-s, -s, s],
                texture_coordinates: [1.0, 1.0],
                normal: [-1.0, 0.0, 0.0],
                tangent: [0.0, 0.0, 1.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            MeshVertex {
                position: [-s, s, s],
                texture_coordinates: [1.0, 0.0],
                normal: [-1.0, 0.0, 0.0],
                tangent: [0.0, 0.0, 1.0],
                bitangent: [0.0, 1.0, 0.0],
            },
            MeshVertex {
                position: [-s, s, -s],
                texture_coordinates: [0.0, 0.0],
                normal: [-1.0, 0.0, 0.0],
                tangent: [0.0, 0.0, 1.0],
                bitangent: [0.0, 1.0, 0.0],
            },
        ];

        // Triangle-list index buffer: each face is two triangles (a,b,c) + (a,c,d)
        // over its 4 vertices, sharing the a-c diagonal — 6 indices per face, 36 total.
        // See: https://learnopengl.com/Getting-started/Hello-Triangle
        let indices = vec![
            0, 1, 2, 0, 2, 3, // front
            4, 5, 6, 4, 6, 7, // back
            8, 9, 10, 8, 10, 11, // top
            12, 13, 14, 12, 14, 15, // bottom
            16, 17, 18, 16, 18, 19, // right
            20, 21, 22, 20, 22, 23, // left
        ];

        MeshData {
            vertices,
            indices,
            texture_coordinates_2: Vec::new(),
        }
    }

    /// Triangulates a closed 2D polyline like [`MeshData::from_polygon`], and also writes a
    /// second UV set (`uv2`, one entry per input point) into `texture_coordinates_2`.
    ///
    /// The values follow the outline through the same normalization the positions do (duplicate
    /// removal and winding), so `uv2[i]` always belongs to `points[i]`.
    pub fn from_polygon_with_uv2(points: &[Vector2f], uv2: &[[f32; 2]]) -> Result<Self> {
        if points.len() != uv2.len() {
            return Err(format!(
                "polygon uv2 length {} does not match point count {}",
                uv2.len(),
                points.len()
            )
            .into());
        }

        let mut mesh = Self::from_polygon(points)?;
        let mut per_vertex = Vec::with_capacity(mesh.vertices.len());
        for vertex in &mesh.vertices {
            let position = Vector2f::new(vertex.position[0], vertex.position[1]);
            let index = points
                .iter()
                .position(|point| point.distance_squared(position) <= 1e-12)
                .ok_or("polygon vertex does not match any input point")?;
            per_vertex.push(uv2[index]);
        }
        mesh.texture_coordinates_2 = per_vertex;
        Ok(mesh)
    }

    /// Builds a quad strip between two polylines with the same number of points: `top[i]` and
    /// `bottom[i]` are connected by an edge, and each quad is split into two triangles.
    ///
    /// Geometry lies at `z = 0` with +Z normals and counter-clockwise winding. UVs run 0..1
    /// across the strip (u) and 0 on the top edge to 1 on the bottom edge (v).
    pub fn from_quad_strip(top: &[Vector2f], bottom: &[Vector2f]) -> Result<Self> {
        Self::from_quad_strip_with_top_uv2(top, bottom, &[])
    }

    /// [`MeshData::from_quad_strip`] plus a second UV set for the top row; bottom-row vertices
    /// get `[0.0, 0.0]`.
    pub fn from_quad_strip_with_top_uv2(
        top: &[Vector2f],
        bottom: &[Vector2f],
        uv2_top: &[[f32; 2]],
    ) -> Result<Self> {
        if top.len() != bottom.len() {
            return Err(format!(
                "quad strip rows differ in length: {} top vs {} bottom",
                top.len(),
                bottom.len()
            )
            .into());
        }
        if top.len() < 2 {
            return Err("quad strip requires at least 2 points per row".into());
        }
        if !uv2_top.is_empty() && uv2_top.len() != top.len() {
            return Err(format!(
                "quad strip uv2 length {} does not match {} points",
                uv2_top.len(),
                top.len()
            )
            .into());
        }

        let count = top.len();
        let inv_u = 1.0 / (count - 1) as f32;
        let mut vertices = Vec::with_capacity(count * 2);
        let mut texture_coordinates_2 = if uv2_top.is_empty() {
            Vec::new()
        } else {
            Vec::with_capacity(count * 2)
        };

        for (point, row) in [(top, 0.0f32), (bottom, 1.0f32)] {
            for (i, position) in point.iter().enumerate() {
                if !position.is_finite() {
                    return Err("quad strip contains a non-finite coordinate".into());
                }
                vertices.push(MeshVertex {
                    position: [position.x, position.y, 0.0],
                    texture_coordinates: [i as f32 * inv_u, row],
                    normal: [0.0, 0.0, 1.0],
                    tangent: [1.0, 0.0, 0.0],
                    bitangent: [0.0, 1.0, 0.0],
                });
                if !uv2_top.is_empty() {
                    // Top row keeps its value, bottom row has no velocity
                    texture_coordinates_2.push(if row == 0.0 { uv2_top[i] } else { [0.0, 0.0] });
                }
            }
        }

        // Two triangles per quad, counter-clockwise when the top row runs left to right
        let mut indices = Vec::with_capacity((count - 1) * 6);
        for i in 0..count - 1 {
            let top_0 = i as u32;
            let top_1 = (i + 1) as u32;
            let bottom_0 = (count + i) as u32;
            let bottom_1 = (count + i + 1) as u32;
            indices.extend_from_slice(&[bottom_0, bottom_1, top_0]);
            indices.extend_from_slice(&[top_0, bottom_1, top_1]);
        }

        Ok(Self {
            vertices,
            indices,
            texture_coordinates_2,
        })
    }

    /// Triangulates a closed 2D polyline into a flat mesh in the XY plane (ear clipping).
    ///
    /// See [`Mesh::from_polygon`] for the accepted input and generated geometry.
    pub fn from_polygon(points: &[Vector2f]) -> Result<Self> {
        let outline = normalize_polygon_outline(points)?;
        let indices = triangulate_polygon(&outline)?;

        let (min, max) = polygon_bounds(&outline);
        let size = max - min;
        // Match the cube convention: u rightwards, v upwards from the bottom edge.
        let inv_size_x = if size.x > f32::EPSILON {
            1.0 / size.x
        } else {
            0.0
        };
        let inv_size_y = if size.y > f32::EPSILON {
            1.0 / size.y
        } else {
            0.0
        };

        let vertices = outline
            .iter()
            .map(|point| {
                let u = if inv_size_x > 0.0 {
                    (point.x - min.x) * inv_size_x
                } else {
                    0.5
                };
                let v = if inv_size_y > 0.0 {
                    1.0 - (point.y - min.y) * inv_size_y
                } else {
                    0.5
                };
                MeshVertex {
                    position: [point.x, point.y, 0.0],
                    texture_coordinates: [u, v],
                    normal: [0.0, 0.0, 1.0],
                    tangent: [1.0, 0.0, 0.0],
                    bitangent: [0.0, 1.0, 0.0],
                }
            })
            .collect();

        Ok(Self {
            vertices,
            indices,
            texture_coordinates_2: Vec::new(),
        })
    }
}

// --- Polygon triangulation ---------------------------------------------------

fn polygon_bounds(points: &[Vector2f]) -> (Vector2f, Vector2f) {
    let mut min = Vector2f::splat(f32::INFINITY);
    let mut max = Vector2f::splat(f32::NEG_INFINITY);
    for point in points {
        min = min.min(*point);
        max = max.max(*point);
    }
    (min, max)
}

/// Removes repeated points (including an optional duplicated closing point), validates the
/// polygon, and normalizes its winding to counter-clockwise.
fn normalize_polygon_outline(points: &[Vector2f]) -> Result<Vec<Vector2f>> {
    if points.len() < 3 {
        return Err("polygon requires at least 3 vertices".into());
    }

    let duplicate_epsilon = 1e-6;
    let mut outline: Vec<Vector2f> = Vec::with_capacity(points.len());
    for &point in points {
        if !point.is_finite() {
            return Err("polygon contains a non-finite coordinate".into());
        }
        let is_duplicate = outline.last().map_or(false, |last| {
            last.distance_squared(point) <= duplicate_epsilon * duplicate_epsilon
        });
        if !is_duplicate {
            outline.push(point);
        }
    }

    // Allow (but don't require) the polyline to repeat its first point at the end.
    if outline.len() > 1 {
        let first = outline[0];
        let last = *outline.last().unwrap();
        if first.distance_squared(last) <= duplicate_epsilon * duplicate_epsilon {
            outline.pop();
        }
    }

    if outline.len() < 3 {
        return Err("polygon requires at least 3 distinct vertices".into());
    }

    let (min, max) = polygon_bounds(&outline);
    let extent_squared = (max - min).length_squared().max(1.0);
    let mut twice_area = 0.0;
    for i in 0..outline.len() {
        let a = outline[i];
        let b = outline[(i + 1) % outline.len()];
        twice_area += a.x * b.y - b.x * a.y;
    }

    if twice_area.abs() <= 1e-9 * extent_squared {
        return Err("polygon is degenerate (zero area)".into());
    }

    if twice_area < 0.0 {
        outline.reverse();
    }

    Ok(outline)
}

/// 2D cross product of `b - a` and `c - a`; positive for a counter-clockwise turn.
fn cross_2d(a: Vector2f, b: Vector2f, c: Vector2f) -> f32 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Strictly-inside test for a counter-clockwise triangle.
fn point_in_triangle(p: Vector2f, a: Vector2f, b: Vector2f, c: Vector2f, epsilon: f32) -> bool {
    cross_2d(a, b, p) > epsilon && cross_2d(b, c, p) > epsilon && cross_2d(c, a, p) > epsilon
}

/// Ear-clipping triangulation of a simple, counter-clockwise polygon.
fn triangulate_polygon(outline: &[Vector2f]) -> Result<Vec<u32>> {
    let vertex_count = outline.len();
    let mut remaining: Vec<usize> = (0..vertex_count).collect();
    let mut indices = Vec::with_capacity((vertex_count - 2) * 3);

    // Scale-relative tolerance: cross products are areas, so compare against the squared extent.
    let (min, max) = polygon_bounds(outline);
    let epsilon = 1e-6 * (max - min).length_squared().max(1.0);

    while remaining.len() > 3 {
        let count = remaining.len();
        let mut ear: Option<(usize, usize, usize, usize)> = None;

        for i in 0..count {
            let prev = remaining[(i + count - 1) % count];
            let current = remaining[i];
            let next = remaining[(i + 1) % count];
            let (a, b, c) = (outline[prev], outline[current], outline[next]);

            // Reflex or collinear corners cannot be ears.
            if cross_2d(a, b, c) <= epsilon {
                continue;
            }

            let contains_vertex = remaining.iter().any(|&index| {
                index != prev
                    && index != current
                    && index != next
                    && point_in_triangle(outline[index], a, b, c, epsilon)
            });

            if !contains_vertex {
                ear = Some((i, prev, current, next));
                break;
            }
        }

        let Some((i, prev, current, next)) = ear else {
            return Err(
                "failed to triangulate polygon: it may be self-intersecting or have holes".into(),
            );
        };

        indices.extend_from_slice(&[prev as u32, current as u32, next as u32]);
        remaining.remove(i);
    }

    let (a, b, c) = (
        outline[remaining[0]],
        outline[remaining[1]],
        outline[remaining[2]],
    );
    if cross_2d(a, b, c) > epsilon {
        indices.extend_from_slice(&[
            remaining[0] as u32,
            remaining[1] as u32,
            remaining[2] as u32,
        ]);
    }

    Ok(indices)
}

#[cfg(test)]
mod polygon_tests {
    use super::*;

    fn outline_area(points: &[Vector2f]) -> f32 {
        let mut twice_area = 0.0;
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            twice_area += a.x * b.y - b.x * a.y;
        }
        twice_area.abs() * 0.5
    }

    fn mesh_area(mesh: &MeshData) -> f32 {
        mesh.indices
            .chunks_exact(3)
            .map(|triangle| {
                let position = |index: u32| {
                    let p = mesh.vertices[index as usize].position;
                    Vector2f::new(p[0], p[1])
                };
                cross_2d(
                    position(triangle[0]),
                    position(triangle[1]),
                    position(triangle[2]),
                )
                .abs()
                    * 0.5
            })
            .sum()
    }

    #[test]
    fn square_is_two_triangles() {
        let points = [
            Vector2f::new(0.0, 0.0),
            Vector2f::new(1.0, 0.0),
            Vector2f::new(1.0, 1.0),
            Vector2f::new(0.0, 1.0),
        ];
        let mesh = MeshData::from_polygon(&points).unwrap();
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.indices.len(), 6);
        assert!((mesh_area(&mesh) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn concave_polygon_is_filled_without_overlap() {
        // Clockwise L-shape with a duplicated closing point; area = 3.
        let points = [
            Vector2f::new(0.0, 0.0),
            Vector2f::new(0.0, 2.0),
            Vector2f::new(1.0, 2.0),
            Vector2f::new(1.0, 1.0),
            Vector2f::new(2.0, 1.0),
            Vector2f::new(2.0, 0.0),
            Vector2f::new(0.0, 0.0),
        ];
        let mesh = MeshData::from_polygon(&points).unwrap();
        assert_eq!(mesh.vertices.len(), 6);
        assert_eq!(mesh.indices.len(), (6 - 2) * 3);
        assert!((mesh_area(&mesh) - outline_area(&points[..6])).abs() < 1e-5);
    }

    #[test]
    fn geometry_is_flat_with_front_facing_normals() {
        let points = [
            Vector2f::new(-1.0, -1.0),
            Vector2f::new(1.0, -1.0),
            Vector2f::new(1.0, 1.0),
            Vector2f::new(-1.0, 1.0),
        ];
        let mesh = MeshData::from_polygon(&points).unwrap();
        for vertex in &mesh.vertices {
            assert_eq!(vertex.position[2], 0.0);
            assert_eq!(vertex.normal, [0.0, 0.0, 1.0]);
        }
        // Triangles are counter-clockwise in the XY plane.
        for triangle in mesh.indices.chunks_exact(3) {
            let p = |index: u32| {
                let v = mesh.vertices[index as usize].position;
                Vector2f::new(v[0], v[1])
            };
            assert!(
                cross_2d(p(triangle[0]), p(triangle[1]), p(triangle[2])) > 0.0,
                "triangle winding must be counter-clockwise"
            );
        }
    }

    #[test]
    fn degenerate_polygons_are_rejected() {
        assert!(MeshData::from_polygon(&[]).is_err());
        assert!(MeshData::from_polygon(&[Vector2f::ZERO, Vector2f::X]).is_err());
        assert!(MeshData::from_polygon(&[
            Vector2f::new(0.0, 0.0),
            Vector2f::new(1.0, 0.0),
            Vector2f::new(2.0, 0.0),
        ])
        .is_err());
    }
}
