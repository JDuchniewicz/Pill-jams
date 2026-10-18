use pill_engine::{define_component, game::*};

use crate::game::GameUIStateComponent;
use crate::water::{GameState, WATER_X_MAX, WATER_X_MIN};

const VERTEX_SHADER: &str = include_str!("../res/shaders/water_mesh_vertex.wgsl");
const FRAGMENT_SHADER: &str = include_str!("../res/shaders/water_mesh_fragment.wgsl");

/// Name of the custom-shader "material" that draws procedural meshes as UV rainbows.
pub const WATER_MESH_SHADER_NAME: &str = "water_mesh_shader";

/// World Y range used by the water's vertical colour gradient.
const WATER_GRADIENT_TOP_Y: f32 = 0.0;
const WATER_GRADIENT_BOTTOM_Y: f32 = -100.0;

/// Colours of the water shading, editable in the "Water look" panel.
/// Mirrors `WaterLook` in water_mesh_fragment.hlsl (linear RGB).
#[derive(Clone, Debug)]
pub struct WaterLookParams {
    /// Water colour at the top of the world gradient (near the surface).
    pub top_color: [f32; 3],
    /// Water colour at the bottom of the world gradient.
    pub bottom_color: [f32; 3],
    /// Overlay colour at the top of the gradient.
    pub highlight_top: [f32; 3],
    /// Overlay colour at the bottom of the gradient.
    pub highlight_bottom: [f32; 3],
    /// The overlay is applied where the (inverted) depth value is above this threshold.
    pub highlight_threshold: f32,
    /// Foam colour, blended on top inside the foam band, weighted by the second UV set.
    pub foam_color: [f32; 3],
    /// Lower bound of the (inverted) depth band where foam appears: higher values are nearer the
    /// surface. The band fades in with `smoothstep` up to the surface.
    pub foam_depth: f32,
    /// Multiplier applied to the second-UV value (horizontal velocity) to scale how much foam
    /// the motion produces — foam is multiplied by the UV value rather than thresholded by it.
    pub foam_intensity: f32,
}

impl Default for WaterLookParams {
    fn default() -> Self {
        Self {
            top_color: [0.15, 0.45, 0.70],
            bottom_color: [0.02, 0.08, 0.20],
            highlight_top: [0.55, 0.85, 0.95],
            highlight_bottom: [0.25, 0.60, 0.80],
            highlight_threshold: 0.95,
            foam_color: [0.95, 0.98, 1.0],
            foam_depth: 0.93,
            foam_intensity: 2.0,
        }
    }
}

const MAX_WATER_MESHES: usize = 16;

/// Maximum number of water surface samples uploaded to the shader (one per simulated point).
const MAX_SURFACE_SAMPLES: usize = 1024;
/// Depth that maps to white in the grayscale output (the water body is ~100 units deep).
const MAX_DEPTH: f32 = 100.0;

const BIND_GROUP_FRAME: usize = 0;

// Entities with this component are drawn by `WaterMeshPass` using `water_mesh_shader`.
define_component!(WaterMeshComponent { mesh: MeshHandle });

/// Mirrors `PerDraw` in water_mesh_vertex.hlsl: position, rotation (radians), scale
type PerDraw = [[f32; 4]; 3];

/// Second UV stream (`TEXCOORD1`) → shader location 8.
fn uv2_layout<'a>() -> wgpu::VertexBufferLayout<'a> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[wgpu::VertexAttribute {
            offset: 0,
            shader_location: 8,
            format: wgpu::VertexFormat::Float32x2,
        }],
    }
}

struct WaterMeshPassState {
    pipeline: Pipeline,
    camera_buffer: wgpu::Buffer,
    per_draw_buffer: wgpu::Buffer,
    /// `SurfaceParams`: count, x_min, x_max, max_depth (bit-packed u32 words), then the
    /// second-UV debug flag and padding.
    surface_params_buffer: wgpu::Buffer,
    /// Water surface heights (world Y), uniformly spaced between x_min and x_max.
    surface_heights_buffer: wgpu::Buffer,
    /// `WaterLook`: gradient and overlay colours (from the egui panel).
    look_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
}

/// Game-defined scene pass: draws `WaterMeshComponent` entities into the engine's HDR and depth
/// targets between the opaque PBR pass and tonemapping.
pub struct WaterMeshPass {
    state: Option<WaterMeshPassState>,
    per_draw_data: Vec<PerDraw>,
    draw_list: Vec<MeshHandle>,
}

impl WaterMeshPass {
    pub fn new() -> Self {
        Self {
            state: None,
            per_draw_data: Vec::with_capacity(MAX_WATER_MESHES),
            draw_list: Vec::with_capacity(MAX_WATER_MESHES),
        }
    }
}

impl Default for WaterMeshPass {
    fn default() -> Self {
        Self::new()
    }
}

impl Pass for WaterMeshPass {
    fn get_label(&self) -> &str {
        WATER_MESH_SHADER_NAME
    }

    fn init(&mut self, renderer: &mut dyn PillRenderer) -> Result<()> {
        let bind_groups = vec![vec![
            // 0: camera uniform
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // 1: per-draw storage
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // 2: water surface params (fragment)
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // 3: water surface heights (fragment)
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // 4: water look colours (fragment)
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ]];

        let pipeline = renderer.create_pipeline(PipelineDesc {
            label: Some(WATER_MESH_SHADER_NAME),
            vs: ShaderDesc {
                source: VERTEX_SHADER,
                entry_func: "vs_main",
            },
            ps: ShaderDesc {
                source: FRAGMENT_SHADER,
                entry_func: "fs_main",
            },
            vertex_buffers: &[RendererMesh::data_layout_descriptor(), uv2_layout()],
            bind_groups,
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_TARGET_FORMAT,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_TARGET_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            primitive: wgpu::PrimitiveState {
                // The procedural polygon can be mirrored by its transform; no culling.
                cull_mode: None,
                ..Default::default()
            },
        })?;

        let camera_buffer = renderer.create_buffer(BufferDesc {
            label: Some("water_mesh_pass_camera"),
            byte_size: std::mem::size_of::<[[f32; 4]; 4]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })?;
        let per_draw_buffer = renderer.create_buffer(BufferDesc {
            label: Some("water_mesh_pass_per_draw"),
            byte_size: (std::mem::size_of::<PerDraw>() * MAX_WATER_MESHES) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        })?;
        let surface_params_buffer = renderer.create_buffer(BufferDesc {
            label: Some("water_mesh_pass_surface_params"),
            byte_size: std::mem::size_of::<[u32; 8]>() as u64, // matches SurfaceParams
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })?;
        let surface_heights_buffer = renderer.create_buffer(BufferDesc {
            label: Some("water_mesh_pass_surface_heights"),
            byte_size: (std::mem::size_of::<f32>() * MAX_SURFACE_SAMPLES) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        })?;
        let look_buffer = renderer.create_buffer(BufferDesc {
            label: Some("water_mesh_pass_look"),
            byte_size: std::mem::size_of::<[[f32; 4]; 7]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        })?;
        let frame_bind_group =
            renderer
                .get_device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("water_mesh_pass_frame_bind_group"),
                    layout: &pipeline.bind_group_layouts[BIND_GROUP_FRAME],
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: camera_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: per_draw_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: surface_params_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: surface_heights_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: look_buffer.as_entire_binding(),
                        },
                    ],
                });

        self.state = Some(WaterMeshPassState {
            pipeline,
            camera_buffer,
            per_draw_buffer,
            surface_params_buffer,
            surface_heights_buffer,
            look_buffer,
            frame_bind_group,
        });
        Ok(())
    }

    fn draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        renderer: &mut dyn PillRenderer,
        _frame_target: &FrameTarget,
        world: &WorldQuery<'_>,
    ) -> Result<()> {
        let state = self
            .state
            .as_ref()
            .expect("WaterMeshPass: init() must run before draw()");

        // Collect draws (TransformComponent rotation is in degrees, the shader expects radians)
        self.per_draw_data.clear();
        self.draw_list.clear();
        let transforms = &world.query::<TransformComponent>()?.data;
        let water_meshes = &world.query::<WaterMeshComponent>()?.data;
        for (transform, water_mesh) in transforms.iter().zip(water_meshes.iter()) {
            let (Some(transform), Some(water_mesh)) = (transform, water_mesh) else {
                continue;
            };
            if self.per_draw_data.len() == MAX_WATER_MESHES {
                break;
            }
            let rotation = transform.rotation;
            self.per_draw_data.push([
                transform.position.extend(0.0).to_array(),
                [
                    rotation.x.to_radians(),
                    rotation.y.to_radians(),
                    rotation.z.to_radians(),
                    0.0,
                ],
                transform.scale.extend(0.0).to_array(),
            ]);
            self.draw_list.push(water_mesh.mesh);
        }
        if self.draw_list.is_empty() {
            return Ok(());
        }

        let (camera, camera_transform) = world.active_camera()?;
        let view_projection = camera_view_projection(camera, camera_transform).to_cols_array_2d();
        let queue = renderer.get_queue();
        queue.write_buffer(
            &state.camera_buffer,
            0,
            bytemuck::bytes_of(&view_projection),
        );
        queue.write_buffer(
            &state.per_draw_buffer,
            0,
            bytemuck::cast_slice(&self.per_draw_data),
        );

        // Water surface heights (world Y), uniformly spaced in world X
        let (mut heights, x_min, x_max) = match world.get_global::<GameState>() {
            Ok(game_state) => (
                game_state
                    .water_points
                    .iter()
                    .take(MAX_SURFACE_SAMPLES)
                    .map(|point| point.y)
                    .collect::<Vec<f32>>(),
                WATER_X_MIN,
                WATER_X_MAX,
            ),
            Err(_) => (vec![0.0, 0.0], WATER_X_MIN, WATER_X_MAX),
        };
        if heights.len() < 2 {
            heights = vec![0.0, 0.0];
        }
        // The egui overlay owns the flag; read it once per frame for the fragment shader.
        let ui_state = world.get_global::<GameUIStateComponent>().ok();
        let show_uv2 = ui_state
            .map(|state| {
                *state
                    .show_uv2
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
            })
            .unwrap_or(false);
        let look = ui_state
            .map(|state| {
                state
                    .water_look
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
            })
            .unwrap_or_default();
        queue.write_buffer(
            &state.surface_heights_buffer,
            0,
            bytemuck::cast_slice(&heights),
        );

        let look_uniform: [[f32; 4]; 7] = [
            [look.top_color[0], look.top_color[1], look.top_color[2], 1.0],
            [
                look.bottom_color[0],
                look.bottom_color[1],
                look.bottom_color[2],
                1.0,
            ],
            [
                look.highlight_top[0],
                look.highlight_top[1],
                look.highlight_top[2],
                1.0,
            ],
            [
                look.highlight_bottom[0],
                look.highlight_bottom[1],
                look.highlight_bottom[2],
                1.0,
            ],
            [
                look.highlight_threshold,
                WATER_GRADIENT_TOP_Y,
                WATER_GRADIENT_BOTTOM_Y,
                0.0,
            ],
            [
                look.foam_color[0],
                look.foam_color[1],
                look.foam_color[2],
                1.0,
            ],
            [look.foam_depth, look.foam_intensity, 0.0, 0.0],
        ];
        queue.write_buffer(&state.look_buffer, 0, bytemuck::bytes_of(&look_uniform));
        let surface_params: [u32; 8] = [
            heights.len() as u32,
            x_min.to_bits(),
            x_max.to_bits(),
            MAX_DEPTH.to_bits(),
            // Non-zero selects the second-UV debug view in the fragment shader
            u32::from(show_uv2),
            0,
            0,
            0,
        ];
        queue.write_buffer(
            &state.surface_params_buffer,
            0,
            bytemuck::bytes_of(&surface_params),
        );

        // Draw on top of the PBR pass output (load, not clear)
        let render_state = world.get_global::<RenderStateComponent>()?;
        let color_view = renderer
            .get_render_target_view(render_state.hdr_target())
            .expect("WaterMeshPass: HDR target missing");
        let depth_view = renderer
            .get_render_target_view(render_state.depth_target())
            .expect("WaterMeshPass: depth target missing");
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("water_mesh_pass_render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        render_pass.set_pipeline(&state.pipeline.pipeline);
        render_pass.set_bind_group(BIND_GROUP_FRAME as u32, &state.frame_bind_group, &[]);

        for (instance, mesh_handle) in self.draw_list.iter().enumerate() {
            let mesh = world.renderer_mesh(mesh_handle)?;
            // The shader requires the second UV stream
            let Some(uv2_buffer) = &mesh.uv2_buffer else {
                continue;
            };
            render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            render_pass.set_vertex_buffer(1, uv2_buffer.slice(..));
            render_pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            let instance = instance as u32;
            render_pass.draw_indexed(0..mesh.index_count, 0, instance..instance + 1);
        }

        Ok(())
    }
}
